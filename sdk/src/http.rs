//! Shared JSON transport and service failure classification.
use crate::protocol::{Settings, bad, bad_request, failure, host, types::Failure};
use serde_json::{Map, Value};

/// Merge `raw`, a JSON object of caller options, into `body`; options may only add fields, and
/// never one named in `reserved`.
///
/// A conflict or invalid JSON fails as `provider.bad_request`.
pub fn apply_options(body: &mut Value, raw: &str, reserved: &[&str]) -> Result<(), Failure> {
    let options: Map<String, Value> =
        serde_json::from_str(raw).map_err(|e| bad_request(format!("invalid options: {e}")))?;
    if let Some(name) = options
        .keys()
        .find(|name| reserved.contains(&name.as_str()))
    {
        return Err(bad_request(format!("options cannot override {name}")));
    }
    merge(body, Value::Object(options))
        .map_err(|path| bad_request(format!("options cannot override {path}")))
}

/// Add `overlay` to `base`, failing wherever both set a value.
///
/// This is the one rule for adding JSON a plugin did not write to JSON it did: objects merge key
/// by key and take the keys `base` lacks, equal-length arrays merge by position, and anything
/// else, `null` included, is a conflict reported by its dotted path.
pub fn merge(base: &mut Value, overlay: Value) -> Result<(), String> {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(slot) => merge(slot, value).map_err(|path| join_path(&key, path))?,
                    None => {
                        base.insert(key, value);
                    }
                }
            }
            Ok(())
        }
        (Value::Array(base), Value::Array(overlay)) if base.len() == overlay.len() => {
            for (index, (slot, value)) in base.iter_mut().zip(overlay).enumerate() {
                merge(slot, value).map_err(|path| join_path(&index.to_string(), path))?;
            }
            Ok(())
        }
        _ => Err(String::new()),
    }
}

fn join_path(segment: &str, below: String) -> String {
    if below.is_empty() {
        segment.into()
    } else {
        format!("{segment}.{below}")
    }
}

/// POST `body` as JSON to `path` under `settings.base_url` and return the JSON reply.
///
/// `None` for `timeout_ms` means the host's default. Retryable: a failed `http` call, 408, 429 and
/// 5xx. Not retryable: 401 and 403 (`provider.auth`), any other status outside 2xx
/// (`provider.bad_request`), and a reply that is not JSON (`provider.bad_response`).
pub async fn post(
    settings: &Settings,
    path: &str,
    body: Value,
    timeout_ms: Option<u32>,
) -> Result<Value, Failure> {
    let response = host::http(host::HttpRequest {
        method: "POST".into(),
        url: format!("{}/{}", settings.base_url.trim_end_matches('/'), path),
        headers: vec![("Content-Type".into(), "application/json".into())],
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
        excerpt.truncate(excerpt.floor_char_boundary(500));
        let (code, retryable) = match response.status {
            401 | 403 => ("provider.auth", false),
            408 => ("timeout", true),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_options_preserve_written_fields_and_report_conflicts() -> Result<(), Failure> {
        let mut body = json!({"generationConfig": {"maxOutputTokens": 100}});
        apply_options(
            &mut body,
            r#"{"generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH"}}}"#,
            &[],
        )?;
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 100);
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "HIGH"
        );
        let conflict = apply_options(
            &mut body,
            r#"{"generationConfig":{"maxOutputTokens":null}}"#,
            &[],
        );
        assert_eq!(
            conflict.map_err(|failure| failure.message),
            Err("options cannot override generationConfig.maxOutputTokens".into())
        );
        assert!(apply_options(&mut body, r#"{"stream":false}"#, &["stream"]).is_err());
        Ok(())
    }
}
