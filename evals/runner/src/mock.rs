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
    header::{CONTENT_TYPE, HeaderValue},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) struct Mock {
    pub(crate) address: SocketAddr,
    state: Arc<Mutex<State>>,
}

struct State {
    routes: Vec<Route>,
    collections: BTreeMap<String, Vec<Value>>,
    requests: Vec<Value>,
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
        root: &Path,
        definition: &task::Mock,
        renderer: &Renderer,
    ) -> Result<Self> {
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
        let mut collections = BTreeMap::new();
        for collection in &definition.collection {
            let values = match &collection.data {
                Some(path) => {
                    let source = tokio::fs::read_to_string(root.join(path)).await?;
                    serde_json::from_str(&renderer.text(&source)?)?
                }
                None => Vec::new(),
            };
            collections.insert(collection.name.clone(), values);
        }
        let state = Arc::new(Mutex::new(State {
            routes,
            collections,
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
        crate::result::write_json(&path.join("state.json"), &state.collections).await?;
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
    for route in &state.routes {
        if route.path == path && route.method == parts.method.as_str() {
            let mut response = reply(route.status, route.body.clone());
            response
                .headers_mut()
                .insert(CONTENT_TYPE, route.content_type.clone());
            return Ok(response);
        }
    }
    let mut segments = path.trim_matches('/').split('/');
    let name = segments.next().unwrap_or("");
    let id = segments.next();
    if segments.next().is_some() {
        return Ok(reply(StatusCode::NOT_FOUND, "not found"));
    }
    let Some(collection) = state.collections.get_mut(name) else {
        return Ok(reply(StatusCode::NOT_FOUND, "not found"));
    };
    let (status, value) = collection_request(collection, parts.method.as_str(), id, &query, body);
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

fn collection_request(
    collection: &mut Vec<Value>,
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
        ("GET", None) => {
            let selected = collection.iter().filter(|item| {
                query.iter().all(|(field, expected)| {
                    field == "fields"
                        || item
                            .get(field)
                            .is_some_and(|value| scalar(value) == *expected)
                })
            });
            let values: Vec<_> = selected
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
                .collect();
            (StatusCode::OK, Value::Array(values))
        }
        ("POST", None) => {
            let Some(object) = body.as_object_mut() else {
                return bad();
            };
            if !object.contains_key("id") {
                let next = collection
                    .iter()
                    .filter_map(|item| item.get("id")?.as_u64())
                    .max()
                    .unwrap_or(0)
                    + 1;
                object.insert("id".into(), next.into());
            }
            collection.push(body.clone());
            (StatusCode::CREATED, body)
        }
        ("GET" | "PATCH" | "DELETE", Some(id)) => {
            let Some(index) = collection
                .iter()
                .position(|item| item.get("id").is_some_and(|value| scalar(value) == id))
            else {
                return (StatusCode::NOT_FOUND, Value::Null);
            };
            match method {
                "GET" => (StatusCode::OK, collection[index].clone()),
                "PATCH" => {
                    let Some(fields) = body.as_object() else {
                        return bad();
                    };
                    let Some(object) = collection[index].as_object_mut() else {
                        return bad();
                    };
                    object.extend(fields.clone());
                    (StatusCode::OK, collection[index].clone())
                }
                _ => {
                    collection.remove(index);
                    (StatusCode::NO_CONTENT, Value::Null)
                }
            }
        }
        _ => (StatusCode::NOT_FOUND, Value::Null),
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
