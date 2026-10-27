//! Bot API envelopes and deterministic HTTP encodings.
use crate::channel::{Fault, Request, Response};
use enco_sdk::types::Failure;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    error_code: Option<u16>,
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u32>,
}

pub(super) fn answer<T: DeserializeOwned>(method: &str, response: Response) -> Result<T, Fault> {
    let envelope = serde_json::from_slice::<Envelope<T>>(&response.body);
    let envelope = match envelope {
        Ok(envelope) => envelope,
        Err(_) if !(200..300).contains(&response.status) => {
            return Err(http_error(method, response.status, None));
        }
        Err(_) => return Err(invalid_response(method)),
    };
    if !envelope.ok || !(200..300).contains(&response.status) {
        let status = if (200..300).contains(&response.status) {
            envelope
                .error_code
                .ok_or_else(|| invalid_response(method))?
        } else {
            response.status
        };
        return Err(http_error(
            method,
            status,
            envelope.parameters.and_then(|p| p.retry_after),
        ));
    }
    envelope.result.ok_or_else(|| invalid_response(method))
}

pub(super) fn request(method: &str, body: Value, timeout_ms: u32) -> Request {
    Request {
        method: "POST".into(),
        url: format!("/bot/{method}"),
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some(body.to_string().into_bytes()),
        timeout_ms: Some(timeout_ms),
    }
}

pub(super) fn http_error(method: &str, status: u16, retry_after: Option<u32>) -> Fault {
    Fault {
        failure: Failure {
            code: format!("channel.http.{status}"),
            message: format!("{method} returned HTTP {status}"),
            retryable: status == 429 || status >= 500,
        },
        // Only a client rejection proves non-delivery: reads may retry a 5xx, sends may not.
        unknown: !(400..500).contains(&status),
        fatal: matches!(status, 401 | 404),
        retry_after,
    }
}

pub(super) fn invalid_response(method: &str) -> Fault {
    Fault {
        failure: Failure {
            code: "channel.bad_response".into(),
            message: format!("{method} returned an invalid response"),
            retryable: false,
        },
        unknown: true,
        fatal: false,
        retry_after: None,
    }
}

pub(super) fn path(path: &str) -> String {
    let mut result = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            result.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            result.push('%');
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    result
}
