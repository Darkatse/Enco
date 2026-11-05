//! Streamed completions: the request that asks for a stream, the server-sent events it brings
//! back, and the failures a stream reports on the way.
//!
//! A stream is only transport. The host reads it whole, and the mapper folds its events into the
//! JSON the same reply has as one response, so one reading follows either way (see [`answer`]).
//!
//! [`answer`]: super::answer
use super::{COMPLETION_TIMEOUT_MS, Failure, HttpRequest, bad, bad_request, failure};
use serde_json::{Map, Value};

/// How long a streamed completion may take in all, in milliseconds.
///
/// Thirty minutes: while bytes keep coming the reply is progressing, and 32k tokens at 20 a
/// second take 27 minutes.
pub const STREAM_TIMEOUT_MS: u32 = 1_800_000;

/// How long a streamed completion may go without a byte, in milliseconds.
///
/// Five minutes, as in codex and rig: a reasoning model that streams no summary can be silent for
/// minutes. A gateway's keepalive comments count as bytes.
pub const STREAM_IDLE_TIMEOUT_MS: u32 = 300_000;

/// One server-sent event: its type, `message` when the stream named none, and its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `event` field, or `message`.
    pub name: String,
    /// The `data` lines joined with `\n`.
    pub data: String,
}

/// Whether the caller's options ask for a stream, and the options without `stream`.
///
/// `stream` is Enco's own option, not a field of the request: `true` unless the owner wrote
/// `false`; anything else is `provider.bad_request`.
pub fn streaming(options: &str) -> Result<(bool, String), Failure> {
    let mut options: Map<String, Value> =
        serde_json::from_str(options).map_err(|e| bad_request(format!("invalid options: {e}")))?;
    let stream = match options.remove("stream") {
        None => true,
        Some(Value::Bool(stream)) => stream,
        Some(_) => return Err(bad_request("the stream option must be true or false")),
    };
    Ok((stream, Value::Object(options).to_string()))
}

/// A completion POST of `body` as JSON to `url`, a path the host joins to the endpoint.
///
/// A stream asks for `text/event-stream` and may take [`STREAM_TIMEOUT_MS`] in all, with at most
/// [`STREAM_IDLE_TIMEOUT_MS`] between bytes; a single reply may take [`COMPLETION_TIMEOUT_MS`].
pub fn post_completion(url: impl Into<String>, body: &Value, stream: bool) -> HttpRequest {
    let mut headers = vec![("content-type".into(), "application/json".into())];
    if stream {
        headers.push(("accept".into(), "text/event-stream".into()));
    }
    HttpRequest {
        method: "POST".into(),
        url: url.into(),
        headers,
        body: Some(body.to_string().into_bytes()),
        timeout_ms: Some(if stream {
            STREAM_TIMEOUT_MS
        } else {
            COMPLETION_TIMEOUT_MS
        }),
        idle_timeout_ms: stream.then_some(STREAM_IDLE_TIMEOUT_MS),
    }
}

/// The events of an SSE body, read by the WHATWG rules.
///
/// Lines end in CRLF, LF or CR; a leading BOM is skipped; a line starting with `:` is a comment;
/// one space after a field's colon is dropped; `data` lines join with `\n`; a blank line
/// dispatches an event that has data. What follows the last line ending is unfinished and is
/// dropped, as is an event the body ends without dispatching. A body that is not UTF-8 is
/// `provider.bad_response`.
pub fn events(body: &[u8]) -> Result<Vec<Event>, Failure> {
    let text = std::str::from_utf8(body)
        .map_err(|e| bad(format!("the event stream is not UTF-8: {e}")))?;
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut events = vec![];
    let (mut name, mut data) = (String::new(), String::new());
    while let Some(end) = rest.find(['\r', '\n']) {
        let line = &rest[..end];
        let after = &rest[end..];
        rest = after.strip_prefix("\r\n").unwrap_or(&after[1..]);
        if line.is_empty() {
            if data.pop().is_some() {
                events.push(Event {
                    name: if name.is_empty() {
                        "message".into()
                    } else {
                        std::mem::take(&mut name)
                    },
                    data: std::mem::take(&mut data),
                });
            }
            name.clear();
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });
        match field {
            "data" => {
                data.push_str(value);
                data.push('\n');
            }
            "event" => name = value.into(),
            _ => {}
        }
    }
    Ok(events)
}

/// The failure an HTTP `status` means, with `message`.
///
/// Retryable: 408 (`timeout`), 429 (`provider.rate_limited`) and 5xx (`provider.server`). Not
/// retryable: 401 and 403 (`provider.auth`) and any other status (`provider.bad_request`). A
/// stream that reports an error after its 200 is classified here by the status its protocol
/// gives that error.
pub fn status_failure(status: u16, message: impl Into<String>) -> Failure {
    let (code, retryable) = match status {
        401 | 403 => ("provider.auth", false),
        408 => ("timeout", true),
        429 => ("provider.rate_limited", true),
        500..=599 => ("provider.server", true),
        _ => ("provider.bad_request", false),
    };
    failure(code, message, retryable)
}

/// A stream that ended before `marker`, its protocol's sign of a complete reply.
///
/// The service cut the reply short, so asking again may succeed: a retryable `provider.server`.
pub fn ended_early(marker: &str) -> Failure {
    failure(
        "provider.server",
        format!("the stream ended before {marker}"),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, data: &str) -> Event {
        Event {
            name: name.into(),
            data: data.into(),
        }
    }

    #[test]
    fn events_follow_the_sse_rules() -> Result<(), Failure> {
        let body = "\u{feff}: keepalive\r\nevent: ping\r\ndata: {\"type\": \"ping\"}\r\n\r\n\
                    data:first\rdata:  second\r\r\
                    id: 7\nretry: 10\ndata\n\n\
                    event: lonely\n\n\
                    data: [DONE]\n\n\
                    data: unfinished\n";
        assert_eq!(
            events(body.as_bytes())?,
            vec![
                event("ping", "{\"type\": \"ping\"}"),
                event("message", "first\n second"),
                event("message", ""),
                event("message", "[DONE]"),
            ]
        );
        assert!(events(&[0xff, b'\n', b'\n']).is_err());
        Ok(())
    }

    #[test]
    fn stream_is_an_option_read_before_the_others() -> Result<(), Failure> {
        assert_eq!(streaming("{}")?, (true, "{}".into()));
        assert_eq!(
            streaming(r#"{"stream":false,"top_p":1}"#)?,
            (false, r#"{"top_p":1}"#.into())
        );
        assert!(streaming(r#"{"stream":"no"}"#).is_err());
        Ok(())
    }
}
