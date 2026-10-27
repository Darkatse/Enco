//! Discord HTTP requests and response evidence at the protocol boundary.
use crate::channel::{Fault, Request, Response};
use enco_sdk::types::Failure;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The time limit of every request but an upload.
const REQUEST_TIMEOUT_MS: u32 = 30_000;

pub(super) fn fault(code: &str, message: impl Into<String>, fatal: bool) -> Fault {
    Fault {
        failure: Failure {
            code: code.into(),
            message: message.into(),
            retryable: false,
        },
        unknown: false,
        fatal,
        retry_after: None,
    }
}

pub(super) fn invalid(context: &str) -> Fault {
    fault(
        "channel.bad_response",
        format!("invalid Discord {context}"),
        false,
    )
}

pub(super) fn request(method: &str, url: String, body: Option<Value>) -> Request {
    Request {
        method: method.into(),
        url,
        headers: body
            .as_ref()
            .map(|_| vec![("content-type".into(), "application/json".into())])
            .unwrap_or_default(),
        body: body.map(|body| body.to_string().into_bytes()),
        timeout_ms: Some(REQUEST_TIMEOUT_MS),
    }
}

pub(super) fn checked(response: &Response) -> Result<(), Fault> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let retry_after = if response.status == 429 {
        #[derive(serde::Deserialize)]
        struct Limit {
            retry_after: f64,
        }
        let limit: Limit =
            serde_json::from_slice(&response.body).map_err(|_| invalid("rate limit"))?;
        if !(0.0..=f64::from(u32::MAX)).contains(&limit.retry_after) {
            return Err(invalid("rate limit retry_after"));
        }
        Some(limit.retry_after.ceil() as u32)
    } else {
        None
    };
    Err(Fault {
        failure: Failure {
            code: format!("channel.http.{}", response.status),
            message: format!("Discord returned HTTP {}", response.status),
            retryable: response.status == 429 || response.status >= 500,
        },
        unknown: !(400..500).contains(&response.status),
        fatal: response.status == 401,
        retry_after,
    })
}

pub(super) fn answer<T: DeserializeOwned>(response: Response) -> Result<T, Fault> {
    checked(&response)?;
    serde_json::from_slice(&response.body).map_err(|_| {
        let mut error = invalid("HTTP response");
        error.unknown = true;
        error
    })
}

pub(super) fn path(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
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
