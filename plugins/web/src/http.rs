use crate::{bad, host, types::Failure};
use serde::de::DeserializeOwned;
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
        idle_timeout_ms: None,
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

/// `body` read as the `T` that states what the service promises; a body that breaks the promise
/// fails, naming `what` it was.
pub(crate) fn decode<T: DeserializeOwned>(what: &str, body: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(body).map_err(|error| {
        if error.is_data() {
            bad(format!("{what} does not match its contract: {error}"))
        } else {
            bad(format!("{what} is not JSON: {error}"))
        }
    })
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
