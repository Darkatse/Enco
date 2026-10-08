//! Shared JSON transport and service failure classification.
use crate::{Settings, bad, failure, host, types::Failure};
use serde_json::Value;

/// Merge service options without replacing fields owned by the typed request.
pub fn apply_options(body: &mut Value, raw: &str, reserved: &[&str]) -> Result<(), Failure> {
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        failure(
            "provider.bad_request",
            format!("invalid options: {e}"),
            false,
        )
    })?;
    let map = value
        .as_object()
        .ok_or_else(|| failure("provider.bad_request", "options must be an object", false))?;
    for (name, value) in map {
        if reserved.contains(&name.as_str()) {
            return Err(failure(
                "provider.bad_request",
                format!("options cannot override {name}"),
                false,
            ));
        }
        body[name] = value.clone();
    }
    Ok(())
}

/// Send one JSON request through the host and classify transport or service failures.
pub async fn post(
    settings: &Settings,
    path: &str,
    body: Value,
    timeout_ms: Option<u32>,
) -> Result<Value, Failure> {
    let mut headers = vec![("Content-Type".into(), "application/json".into())];
    if let Some(key) = &settings.api_key {
        headers.push(("Authorization".into(), format!("Bearer {key}")));
    }
    let response = host::http(host::HttpRequest {
        method: "POST".into(),
        url: format!("{}/{}", settings.base_url.trim_end_matches('/'), path),
        headers,
        body: Some(body.to_string().into_bytes()),
        timeout_ms,
    })
    .await
    .map_err(|e| {
        failure(
            if e.kind == host::HttpErrorKind::Timeout {
                "timeout"
            } else {
                "provider.network"
            },
            e.message,
            true,
        )
    })?;
    if !(200..300).contains(&response.status) {
        let text = String::from_utf8_lossy(&response.body);
        let mut excerpt = text.into_owned();
        if let Some(key) = &settings.api_key
            && !key.is_empty()
        {
            excerpt = excerpt.replace(key, "[redacted]");
        }
        excerpt.truncate(excerpt.floor_char_boundary(500));
        let (code, retryable) = match response.status {
            401 | 403 => ("provider.auth", false),
            429 => ("provider.rate_limited", true),
            500..=599 => ("provider.server", true),
            _ => ("provider.bad_request", false),
        };
        return Err(failure(
            code,
            format!("HTTP {}: {excerpt}", response.status),
            retryable,
        ));
    }
    serde_json::from_slice(&response.body).map_err(|e| bad(format!("invalid JSON response: {e}")))
}
