//! A streamed `generateContent` reply, folded into the response it is in one piece.
use enco_sdk::protocol::{
    CompleteError, bad, carries,
    stream::{Event, ended_early, status_failure},
    types::Failure,
};
use serde_json::{Map, Value};

type Object = Map<String, Value>;

/// The response `events`, each a `GenerateContentResponse` chunk, spell out.
///
/// The parts of the candidate's content keep their arrival order. Adjacent parts that hold only
/// text, with the same `thought`, merge; a signed part never merges, since a signature may arrive
/// on a part of empty text and belongs where it came. A part with nothing but empty text says
/// nothing and is dropped. The candidate's other fields and the response's, such as
/// `usageMetadata`, take the last value given. The reply is complete once the candidate gives its
/// `finishReason`, or when a prompt was blocked before any candidate came. A chunk with an
/// `error` fails as the HTTP status of its `code`.
pub fn fold(events: Vec<Event>) -> Result<Value, CompleteError> {
    response(events).map_err(CompleteError::Failed)
}

fn response(events: Vec<Event>) -> Result<Value, Failure> {
    let mut response = Object::new();
    let mut candidate: Option<Object> = None;
    let mut content: Option<Object> = None;
    let mut parts = vec![];
    for event in events {
        let mut chunk: Object =
            serde_json::from_str(&event.data).map_err(|e| bad(format!("invalid chunk: {e}")))?;
        if let Some(error) = chunk.remove("error").filter(carries) {
            return Err(reported(&error));
        }
        let mut candidates = match chunk.remove("candidates") {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(candidates)) => candidates,
            Some(_) => return Err(bad("candidates must be a list")),
        };
        response.extend(chunk);
        let Some(given) = candidates.pop() else {
            continue;
        };
        let (Value::Object(mut given), true) = (given, candidates.is_empty()) else {
            return Err(bad("a stream may carry one candidate"));
        };
        match given.remove("content") {
            None | Some(Value::Null) => {}
            Some(Value::Object(mut given)) => {
                match given.remove("parts") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(given)) => {
                        given.into_iter().for_each(|part| add(&mut parts, part))
                    }
                    Some(_) => return Err(bad("parts must be a list")),
                }
                content.get_or_insert_default().extend(given);
            }
            Some(_) => return Err(bad("content must be an object")),
        }
        candidate.get_or_insert_default().extend(given);
    }
    let Some(mut candidate) = candidate else {
        if response.contains_key("promptFeedback") {
            return Ok(Value::Object(response));
        }
        return Err(ended_early("finishReason"));
    };
    if !candidate.contains_key("finishReason") {
        return Err(ended_early("finishReason"));
    }
    if let Some(mut content) = content {
        content.insert("parts".into(), Value::Array(parts));
        candidate.insert("content".into(), Value::Object(content));
    }
    response.insert(
        "candidates".into(),
        Value::Array(vec![Value::Object(candidate)]),
    );
    Ok(Value::Object(response))
}

fn add(parts: &mut Vec<Value>, part: Value) {
    if let Value::Object(given) = &part {
        if given.len() == 1 && given.get("text").and_then(Value::as_str) == Some("") {
            return;
        }
        if let Some(Value::Object(last)) = parts.last_mut()
            && plain(last)
            && plain(given)
            && last.get("thought") == given.get("thought")
            && let (Some(Value::String(text)), Some(more)) = (
                last.get_mut("text"),
                given.get("text").and_then(Value::as_str),
            )
        {
            text.push_str(more);
            return;
        }
    }
    parts.push(part);
}

/// Whether a part holds only text, perhaps marked as a thought, and so may merge with its
/// neighbor.
fn plain(part: &Object) -> bool {
    part.get("text").is_some_and(Value::is_string)
        && part.keys().all(|key| key == "text" || key == "thought")
}

/// The failure an `error` in a chunk reports, by the HTTP status in its `code`.
fn reported(error: &Value) -> Failure {
    let message = format!("the stream reported {error}");
    match error
        .get("code")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
    {
        Some(status) => status_failure(status, message),
        None => bad(message),
    }
}
