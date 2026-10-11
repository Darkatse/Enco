use crate::{
    mock::write_lines,
    server::{Reply, Servers, reply},
};
use anyhow::Result;
use http_body_util::BodyExt;
use hyper::{Method, Request, StatusCode, body::Incoming};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) struct Proxy {
    pub(crate) address: SocketAddr,
    rejected: Arc<Mutex<Vec<Value>>>,
}

impl Proxy {
    pub(crate) async fn start(
        servers: &mut Servers,
        routes: BTreeMap<String, SocketAddr>,
    ) -> Result<Self> {
        let routes = Arc::new(routes);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let rejected = Arc::new(Mutex::new(Vec::new()));
        let log = rejected.clone();
        let address = servers
            .start(move |request| {
                let forwarded = forward(routes.clone(), log.clone(), client.clone(), request);
                async move {
                    Ok(forwarded
                        .await
                        .unwrap_or_else(|_| reply(StatusCode::BAD_GATEWAY, "Bad Gateway")))
                }
            })
            .await?;
        Ok(Self { address, rejected })
    }

    pub(crate) async fn export(&self, path: &Path) -> Result<usize> {
        let rejected = self.rejected.lock().await;
        write_lines(path, &rejected).await?;
        Ok(rejected.len())
    }
}

async fn forward(
    routes: Arc<BTreeMap<String, SocketAddr>>,
    rejected: Arc<Mutex<Vec<Value>>>,
    client: reqwest::Client,
    request: Request<Incoming>,
) -> Result<Reply> {
    let destination = request
        .uri()
        .host()
        .and_then(|host| routes.get(host))
        .filter(|_| {
            request.method() != Method::CONNECT && request.uri().scheme_str() == Some("http")
        });
    let Some(destination) = destination else {
        rejected.lock().await.push(json!({ "at": chrono::Utc::now(), "method": request.method().as_str(), "target": request.uri().to_string() }));
        return Ok(reply(
            StatusCode::FORBIDDEN,
            "<html><body>Request blocked by proxy policy.</body></html>",
        ));
    };
    let (parts, body) = request.into_parts();
    let path = parts.uri.path_and_query().map_or("/", |path| path.as_str());
    let bytes = body.collect().await?.to_bytes();
    let response = client
        .request(parts.method, format!("http://{destination}{path}"))
        .headers(parts.headers)
        .body(bytes)
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut response = reply(status, response.bytes().await?);
    *response.headers_mut() = headers;
    Ok(response)
}
