//! A streamed Messages reply, folded into the message it is in one piece.
use enco_sdk::protocol::{
    CompleteError, bad,
    stream::{Event, ended_early, status_failure},
    types::Failure,
};
use serde_json::{Map, Value};
use std::collections::HashMap;

type Object = Map<String, Value>;

/// The message `events` spell out, as the service returns it unstreamed.
///
/// `message_start` gives the message without content. Each `content_block_start` appends its
/// block in index order, and each `content_block_delta` adds to it: text, thinking and signature
/// append, a citation joins `citations`, and `input_json_delta` buffers a call's input until
/// `content_block_stop` parses it, nothing meaning `{}`. A block that never stops, as a call cut
/// by the output limit, keeps what its start gave, as the unstreamed reply does. `message_delta`
/// sets its fields and updates the usage, whose counts are cumulative. `message_stop` completes
/// the message; a stream that ends before it may be retried. `ping` and event types the service
/// adds later are skipped, while an unknown kind of delta is `provider.bad_response`. An `error`
/// event fails as the HTTP status its type stands for.
pub fn fold(events: Vec<Event>) -> Result<Value, CompleteError> {
    message(events).map_err(CompleteError::Failed)
}

fn message(events: Vec<Event>) -> Result<Value, Failure> {
    let mut message = None;
    let mut inputs = HashMap::new();
    for event in events {
        let mut data: Object =
            serde_json::from_str(&event.data).map_err(|e| bad(format!("invalid event: {e}")))?;
        let kind = data.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "message_start" => match data.remove("message") {
                Some(Value::Object(start)) if message.is_none() => message = Some(start),
                _ => return Err(bad("message_start must start the message once")),
            },
            "content_block_start" => {
                let index = index(&data)?;
                let block = data
                    .remove("content_block")
                    .ok_or_else(|| bad("content_block_start without its block"))?;
                let content = content(&mut message)?;
                if index != content.len() {
                    return Err(bad("content blocks must start in index order"));
                }
                content.push(block);
            }
            "content_block_delta" => {
                let index = index(&data)?;
                let Some(Value::Object(delta)) = data.remove("delta") else {
                    return Err(bad("content_block_delta without its delta"));
                };
                let block = block(&mut message, index)?;
                add(block, inputs.entry(index).or_default(), delta)?;
            }
            "content_block_stop" => {
                let index = index(&data)?;
                let input: String = inputs.remove(&index).unwrap_or_default();
                if !input.is_empty() {
                    let input = serde_json::from_str(&input)
                        .map_err(|e| bad(format!("invalid tool input: {e}")))?;
                    block(&mut message, index)?.insert("input".into(), input);
                }
            }
            "message_delta" => {
                let started = started(&mut message)?;
                if let Some(Value::Object(delta)) = data.remove("delta") {
                    started.extend(delta);
                }
                if let Some(Value::Object(usage)) = data.remove("usage") {
                    match started.get_mut("usage") {
                        Some(Value::Object(counted)) => counted.extend(usage),
                        _ => {
                            started.insert("usage".into(), Value::Object(usage));
                        }
                    }
                }
            }
            "message_stop" => {
                return message
                    .map(Value::Object)
                    .ok_or_else(|| bad("message_stop before message_start"));
            }
            "error" => return Err(reported(data.get("error"))),
            _ => {}
        }
    }
    Err(ended_early("message_stop"))
}

/// Add one delta to `block`, buffering a call's input in `input`.
fn add(block: &mut Object, input: &mut String, mut delta: Object) -> Result<(), Failure> {
    let kind = delta
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let (field, value) = match kind.as_str() {
        "text_delta" => ("text", delta.remove("text")),
        "thinking_delta" => ("thinking", delta.remove("thinking")),
        "signature_delta" => ("signature", delta.remove("signature")),
        "input_json_delta" => {
            let partial = delta.remove("partial_json");
            input.push_str(partial.as_ref().and_then(Value::as_str).unwrap_or_default());
            return Ok(());
        }
        "citations_delta" => {
            let citation = delta
                .remove("citation")
                .ok_or_else(|| bad("citations_delta without its citation"))?;
            match block.get_mut("citations") {
                Some(Value::Array(citations)) => citations.push(citation),
                _ => {
                    block.insert("citations".into(), Value::Array(vec![citation]));
                }
            }
            return Ok(());
        }
        _ => return Err(bad(format!("unknown delta type {kind:?}"))),
    };
    let Some(Value::String(more)) = value else {
        return Err(bad(format!("{kind} without its {field}")));
    };
    match block.get_mut(field) {
        Some(Value::String(text)) => text.push_str(&more),
        _ => {
            block.insert(field.into(), Value::String(more));
        }
    }
    Ok(())
}

fn index(data: &Object) -> Result<usize, Failure> {
    data.get("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| bad("a content block event without its index"))
}

fn started(message: &mut Option<Object>) -> Result<&mut Object, Failure> {
    message
        .as_mut()
        .ok_or_else(|| bad("an event before message_start"))
}

fn content(message: &mut Option<Object>) -> Result<&mut Vec<Value>, Failure> {
    started(message)?
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| bad("message_start without its content list"))
}

fn block(message: &mut Option<Object>, index: usize) -> Result<&mut Object, Failure> {
    content(message)?
        .get_mut(index)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| bad(format!("no content block {index}")))
}

/// The failure an `error` event reports, by the HTTP status its type stands for.
fn reported(error: Option<&Value>) -> Failure {
    let kind = error
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str);
    let status = match kind {
        Some("invalid_request_error") => 400,
        Some("authentication_error") => 401,
        Some("permission_error") => 403,
        Some("not_found_error") => 404,
        Some("request_too_large") => 413,
        Some("rate_limit_error") => 429,
        Some("api_error") => 500,
        Some("overloaded_error") => 529,
        _ => {
            return bad(format!(
                "the stream reported {}",
                error.unwrap_or(&Value::Null)
            ));
        }
    };
    status_failure(
        status,
        format!("the stream reported {}", error.unwrap_or(&Value::Null)),
    )
}
