//! Jina's JSON surface over the trial's existing pages; it cannot contact any other host.
use crate::server::{Reply, Servers, reply};
use anyhow::Result;
use hyper::{Request, StatusCode, body::Incoming, header::CONTENT_TYPE};
use serde_json::json;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

pub(crate) const HOST: &str = "r.jina.ai";
pub(crate) const URL: &str = "http://r.jina.ai";

pub(crate) async fn start(
    servers: &mut Servers,
    pages: BTreeMap<String, SocketAddr>,
) -> Result<SocketAddr> {
    let pages = Arc::new(pages);
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    servers
        .start(move |request| read(client.clone(), pages.clone(), request))
        .await
}

async fn read(
    client: reqwest::Client,
    pages: Arc<BTreeMap<String, SocketAddr>>,
    request: Request<Incoming>,
) -> Result<Reply> {
    let target = request
        .uri()
        .path_and_query()
        .map_or("", |path| path.as_str())
        .trim_start_matches('/');
    let url = match reqwest::Url::parse(target) {
        Ok(url) => url,
        Err(_) => return Ok(reply(StatusCode::BAD_REQUEST, "invalid page URL")),
    };
    let Some(address) = url.host_str().and_then(|host| pages.get(host)) else {
        return Ok(page(
            target,
            StatusCode::NOT_FOUND,
            "Page unavailable".into(),
        ));
    };
    let mut local = reqwest::Url::parse(&format!("http://{address}"))?;
    local.set_path(url.path());
    local.set_query(url.query());
    let response = client.get(local).send().await?;
    let status = response.status();
    Ok(page(target, status, response.text().await?))
}

fn page(url: &str, status: StatusCode, content: String) -> Reply {
    let mut response = reply(StatusCode::OK, json!({"code": 200, "data": {"url": url, "httpStatus": status.as_u16(), "content": content}}).to_string());
    response.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_only_trial_pages_and_preserves_their_status() -> Result<()> {
        let mut servers = Servers::default();
        let origin = servers
            .start(|request| async move {
                Ok(reply(
                    StatusCode::FORBIDDEN,
                    format!("page {}", request.uri()),
                ))
            })
            .await?;
        let reader = start(&mut servers, BTreeMap::from([("news.test".into(), origin)])).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let read = client
            .get(format!("http://{reader}/http://news.test/story?lang=en"))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        assert_eq!(read["data"]["httpStatus"], 403);
        assert_eq!(read["data"]["content"], "page /story?lang=en");
        let absent = client
            .get(format!("http://{reader}/https://outside.test/private"))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        assert_eq!(absent["data"]["httpStatus"], 404);
        servers.stop().await
    }
}
