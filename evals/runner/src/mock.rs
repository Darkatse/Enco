use crate::{
    render::Renderer,
    server::{Reply, Servers, reply},
    task,
};
use anyhow::{Context, Result};
use http_body_util::BodyExt;
use hyper::{
    Request, StatusCode,
    body::{Bytes, Incoming},
    header::{CONTENT_TYPE, HeaderName, HeaderValue},
};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) struct Mock {
    pub(crate) address: SocketAddr,
    state: Arc<Mutex<State>>,
}

struct State {
    routes: Vec<Route>,
    collections: Vec<Collection>,
    /// The header every request must carry, and its exact value.
    auth: Option<(HeaderName, String)>,
    requests: Vec<Value>,
}

/// A collection's items and the rules its service follows, both fixed by the task.
struct Collection {
    name: String,
    path: String,
    items: Vec<Value>,
    /// Fields the service fills in when a created item leaves them out.
    defaults: Map<String, Value>,
    /// The query parameters a listing matches items by; the service ignores any other.
    filters: Vec<String>,
}

struct Route {
    method: String,
    path: String,
    status: StatusCode,
    content_type: HeaderValue,
    body: Bytes,
}

impl Mock {
    pub(crate) async fn start(
        servers: &mut Servers,
        task: &task::Task,
        definition: &task::Mock,
        renderer: &Renderer,
    ) -> Result<Self> {
        let root = &task.root;
        let mut routes = Vec::new();
        for route in &definition.route {
            let source = tokio::fs::read_to_string(root.join(&route.body)).await?;
            routes.push(Route {
                method: route.method.clone(),
                path: route.path.clone(),
                status: route.status,
                content_type: route.content_type.clone(),
                body: renderer.text(&source)?.into(),
            });
        }
        let mut collections = Vec::new();
        for collection in &definition.collection {
            let items = match &collection.data {
                Some(path) => {
                    let source = tokio::fs::read_to_string(root.join(path)).await?;
                    serde_json::from_str(&renderer.text(&source)?)?
                }
                None => Vec::new(),
            };
            collections.push(Collection {
                name: collection.name.clone(),
                path: collection.path(),
                items,
                defaults: collection.defaults.clone(),
                filters: collection.filters.clone(),
            });
        }
        let auth = match &definition.auth {
            Some(auth) => {
                let value = task
                    .definition
                    .environment
                    .variables
                    .get(&auth.variable)
                    .with_context(|| format!("auth variable {}", auth.variable))?;
                Some((
                    HeaderName::from_bytes(auth.header.as_bytes())?,
                    format!("{}{value}", auth.prefix),
                ))
            }
            None => None,
        };
        let state = Arc::new(Mutex::new(State {
            routes,
            collections,
            auth,
            requests: Vec::new(),
        }));
        let handler = state.clone();
        let address = servers
            .start(move |request| handle(handler.clone(), request))
            .await?;
        Ok(Self { address, state })
    }

    pub(crate) async fn export(&self, path: &Path) -> Result<()> {
        let state = self.state.lock().await;
        let collections: BTreeMap<_, _> = state
            .collections
            .iter()
            .map(|collection| (&collection.name, &collection.items))
            .collect();
        crate::result::write_json(&path.join("state.json"), &collections).await?;
        write_lines(&path.join("requests.jsonl"), &state.requests).await
    }
}

async fn handle(state: Arc<Mutex<State>>, request: Request<Incoming>) -> Result<Reply> {
    let (parts, body) = request.into_parts();
    let bytes = body.collect().await?.to_bytes();
    let body = decode(
        &bytes,
        parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    );
    let path = parts.uri.path();
    let query: BTreeMap<String, String> =
        form_urlencoded::parse(parts.uri.query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let mut state = state.lock().await;
    state.requests.push(json!({ "at": chrono::Utc::now(), "method": parts.method.as_str(), "path": path, "query": query, "body": body }));
    if let Some((header, expected)) = &state.auth
        && parts.headers.get(header).and_then(|v| v.to_str().ok()) != Some(expected.as_str())
    {
        let mut response = reply(
            StatusCode::UNAUTHORIZED,
            json!({ "error": "unauthorized", "message": "Missing or invalid access token" })
                .to_string(),
        );
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        return Ok(response);
    }
    for route in &state.routes {
        if route.path == path && route.method == parts.method.as_str() {
            let mut response = reply(route.status, route.body.clone());
            response
                .headers_mut()
                .insert(CONTENT_TYPE, route.content_type.clone());
            return Ok(response);
        }
    }
    let Some((collection, id)) = state.collections.iter_mut().find_map(|collection| {
        let rest = path.strip_prefix(collection.path.as_str())?;
        match rest.strip_prefix('/') {
            None if rest.is_empty() => Some((collection, None)),
            Some(id) if !id.is_empty() && !id.contains('/') => Some((collection, Some(id))),
            _ => None,
        }
    }) else {
        return Ok(reply(StatusCode::NOT_FOUND, "not found"));
    };
    let (status, value) = collection.request(parts.method.as_str(), id, &query, body);
    if status == StatusCode::NO_CONTENT {
        return Ok(reply(status, Bytes::new()));
    }
    let mut response = reply(status, serde_json::to_vec(&value)?);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(response)
}

fn decode(bytes: &[u8], content_type: &str) -> Value {
    if content_type.starts_with("application/json") {
        if let Ok(value) = serde_json::from_slice(bytes) {
            return value;
        }
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        return Value::Object(
            form_urlencoded::parse(bytes)
                .into_owned()
                .map(|(key, value)| (key, Value::String(value)))
                .collect(),
        );
    }
    Value::String(String::from_utf8_lossy(bytes).into_owned())
}

impl Collection {
    fn request(
        &mut self,
        method: &str,
        id: Option<&str>,
        query: &BTreeMap<String, String>,
        mut body: Value,
    ) -> (StatusCode, Value) {
        let bad = || {
            (
                StatusCode::BAD_REQUEST,
                json!({ "error": "expected an object" }),
            )
        };
        match (method, id) {
            ("GET", None) => (StatusCode::OK, Value::Array(self.list(query))),
            ("POST", None) => {
                let Some(object) = body.as_object_mut() else {
                    return bad();
                };
                for (field, value) in &self.defaults {
                    object
                        .entry(field.as_str())
                        .or_insert_with(|| value.clone());
                }
                if !object.contains_key("id") {
                    let next = self
                        .items
                        .iter()
                        .filter_map(|item| item.get("id")?.as_u64())
                        .max()
                        .unwrap_or(0)
                        + 1;
                    object.insert("id".into(), next.into());
                }
                self.items.push(body.clone());
                (StatusCode::CREATED, body)
            }
            ("GET" | "PATCH" | "DELETE", Some(id)) => {
                let Some(index) = self
                    .items
                    .iter()
                    .position(|item| item.get("id").is_some_and(|value| scalar(value) == id))
                else {
                    return (StatusCode::NOT_FOUND, Value::Null);
                };
                match method {
                    "GET" => (StatusCode::OK, self.items[index].clone()),
                    "PATCH" => {
                        let Some(fields) = body.as_object() else {
                            return bad();
                        };
                        let Some(object) = self.items[index].as_object_mut() else {
                            return bad();
                        };
                        object.extend(fields.clone());
                        (StatusCode::OK, self.items[index].clone())
                    }
                    _ => {
                        self.items.remove(index);
                        (StatusCode::NO_CONTENT, Value::Null)
                    }
                }
            }
            _ => (StatusCode::NOT_FOUND, Value::Null),
        }
    }

    /// The items that match every declared filter the query sets, limited to `fields` if given.
    fn list(&self, query: &BTreeMap<String, String>) -> Vec<Value> {
        let selected = self.items.iter().filter(|item| {
            self.filters
                .iter()
                .filter_map(|field| Some((field, query.get(field)?)))
                .all(|(field, expected)| {
                    item.get(field)
                        .is_some_and(|value| scalar(value) == *expected)
                })
        });
        selected
            .map(|item| match query.get("fields") {
                Some(fields) => Value::Object(
                    fields
                        .split(',')
                        .filter_map(|field| {
                            item.get(field)
                                .map(|value| (field.to_owned(), value.clone()))
                        })
                        .collect(),
                ),
                None => item.clone(),
            })
            .collect()
    }
}

fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

pub(crate) async fn write_lines(path: &Path, values: &[Value]) -> Result<()> {
    let mut lines = Vec::new();
    for value in values {
        serde_json::to_writer(&mut lines, value)?;
        lines.push(b'\n');
    }
    tokio::fs::write(path, lines)
        .await
        .with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_follow_the_declared_filters_and_creation_fills_defaults() {
        let mut tasks = Collection {
            name: "tasks".into(),
            path: "/v1/tasks".into(),
            items: vec![json!({ "id": 41, "title": "Renew passport", "status": "completed" })],
            defaults: Map::from_iter([
                ("status".to_owned(), json!("open")),
                ("list_id".to_owned(), json!(7)),
            ]),
            filters: vec!["status".into(), "due_date".into()],
        };
        let none = BTreeMap::new();
        let (status, created) = tasks.request(
            "POST",
            None,
            &none,
            json!({ "title": "Call the landlord", "list_id": 9 }),
        );
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            created,
            json!({ "id": 42, "title": "Call the landlord", "status": "open", "list_id": 9 })
        );
        let query = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(field, value)| ((*field).to_owned(), (*value).to_owned()))
                .collect()
        };
        // An undeclared parameter is ignored; a declared one filters even when no item has it.
        assert_eq!(
            tasks.list(&query(&[("status", "open"), ("limit", "50")])),
            vec![created]
        );
        assert!(tasks.list(&query(&[("due_date", "2026-10-26")])).is_empty());
    }
}
