//! A streamed Responses reply: its terminal event carries the whole response.
use enco_sdk::protocol::{
    CompleteError, Overflow, bad,
    stream::{Event, ended_early},
    types::Failure,
};
use serde_json::Value;

/// The response `events` end with.
///
/// `response.completed` and `response.incomplete` carry the whole response, as the service returns
/// it unstreamed; the events before them are not needed and stay in the record. A stream that ends
/// without one may be retried. `response.failed` and an `error` event fail by their code (see
/// [`reported`]).
pub fn fold(events: Vec<Event>) -> Result<Value, CompleteError> {
    for event in events {
        let mut data: Value =
            serde_json::from_str(&event.data).map_err(|e| failed(format!("invalid event: {e}")))?;
        match data.get("type").and_then(Value::as_str) {
            Some("response.completed" | "response.incomplete") => {
                return data
                    .get_mut("response")
                    .map(Value::take)
                    .ok_or_else(|| failed("a terminal event without its response"));
            }
            Some("response.failed") => return Err(reported(data.pointer("/response/error"))),
            Some("error") => return Err(reported(Some(&data))),
            _ => {}
        }
    }
    Err(CompleteError::Failed(ended_early("response.completed")))
}

/// The failure an error a stream reports means, by its `code`: `context_length_exceeded` is an
/// overflow without numbers; `server_error` and `rate_limit_exceeded` may be retried; any other is
/// `provider.bad_response`, as a failed response is when it comes whole.
fn reported(error: Option<&Value>) -> CompleteError {
    let error = error.unwrap_or(&Value::Null);
    let message = format!("the stream reported {error}");
    let (code, retryable) = match error.get("code").and_then(Value::as_str) {
        Some("context_length_exceeded") => {
            return CompleteError::Overflow(Overflow {
                counted: None,
                accepted: None,
                message,
            });
        }
        Some("server_error") => ("provider.server", true),
        Some("rate_limit_exceeded") => ("provider.rate_limited", true),
        _ => ("provider.bad_response", false),
    };
    CompleteError::Failed(Failure {
        code: code.into(),
        message,
        retryable,
    })
}

fn failed(message: impl Into<String>) -> CompleteError {
    CompleteError::Failed(bad(message))
}
