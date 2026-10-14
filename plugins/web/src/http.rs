use crate::{bad, host, types::Failure};
use serde_json::Value;

const ERROR_PREVIEW_BYTES: usize = 500;

pub(crate) async fn request(
    method: &str,
    url: String,
    body: Option<Value>,
) -> Result<host::HttpResponse, Failure> {
    let response = host::http(host::HttpRequest {
        method: method.into(),
        url,
        headers: vec![
            ("accept".into(), "application/json".into()),
            ("content-type".into(), "application/json".into()),
        ],
        body: body.map(|body| body.to_string().into_bytes()),
        timeout_ms: None,
    })
    .await
    .map_err(|error| bad(format!("HTTP request failed: {}", excerpt(&error.message))))?;
    if !(200..300).contains(&response.status) {
        return Err(bad(format!(
            "HTTP {}: {}",
            response.status,
            excerpt(&String::from_utf8_lossy(&response.body))
        )));
    }
    Ok(response)
}

pub(crate) async fn json(method: &str, url: String, body: Option<Value>) -> Result<Value, Failure> {
    decode(&request(method, url, body).await?)
}

pub(crate) fn decode(response: &host::HttpResponse) -> Result<Value, Failure> {
    serde_json::from_slice(&response.body)
        .map_err(|error| bad(format!("response is not JSON: {error}")))
}

pub(crate) fn excerpt(text: &str) -> String {
    if text.len() <= ERROR_PREVIEW_BYTES {
        return text.into();
    }
    format!(
        "{} [truncated]",
        &text[..text.floor_char_boundary(ERROR_PREVIEW_BYTES)]
    )
}

pub(crate) fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, Failure> {
    value[field]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| bad(format!("response has no {field}")))
}
