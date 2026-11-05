//! A streamed Chat Completions reply, folded into the response it is in one piece.
use crate::protocol::{
    CompleteError, bad, carries,
    stream::{Event, ended_early, status_failure},
};
use crate::types::Failure;
use serde_json::{Map, Value, json};

type Object = Map<String, Value>;

/// The `chat.completion` that `events`, a stream of `chat.completion.chunk`s, spell out.
///
/// Each chunk carries at most one choice, at index 0, whose delta adds to the message: text such as
/// `content` and `reasoning_content` appends, a list extends, and each tool call gathers its pieces
/// by `index`, its `arguments` appending. `role` and a call's `id`, `type` and `name` are given
/// once; a stream that repeats one with another value is `provider.bad_response`. The reply is
/// complete once the choice gives its `finish_reason`; `[DONE]` may follow and ends the reading.
/// `usage` is the last one given. An `error` in a chunk fails as the HTTP status of its numeric
/// `code`, and is otherwise `provider.bad_response`.
pub(super) fn fold(events: Vec<Event>) -> Result<Value, CompleteError> {
    reply(events).map_err(CompleteError::Failed)
}

fn reply(events: Vec<Event>) -> Result<Value, Failure> {
    let mut message = Object::new();
    let mut calls: Vec<Object> = vec![];
    let (mut finish, mut usage) = (None, None);
    for event in events {
        if event.data == "[DONE]" {
            break;
        }
        let mut chunk: Object =
            serde_json::from_str(&event.data).map_err(|e| bad(format!("invalid chunk: {e}")))?;
        if let Some(error) = chunk.remove("error").filter(carries) {
            return Err(reported(&error));
        }
        if let Some(given) = chunk.remove("usage").filter(|usage| !usage.is_null()) {
            usage = Some(given);
        }
        let mut choices = match chunk.remove("choices") {
            Some(Value::Array(choices)) => choices,
            _ => return Err(bad("a chunk without choices")),
        };
        let Some(choice) = choices.pop() else {
            continue;
        };
        let Value::Object(mut choice) = choice else {
            return Err(bad("a choice must be an object"));
        };
        if !choices.is_empty() || choice.get("index").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return Err(bad("a stream may carry only the choice at index 0"));
        }
        if let Some(Value::String(reason)) = choice.remove("finish_reason") {
            finish = Some(reason);
        }
        match choice.remove("delta") {
            None | Some(Value::Null) => {}
            Some(Value::Object(delta)) => add(&mut message, &mut calls, delta)?,
            Some(_) => return Err(bad("a delta must be an object")),
        }
    }
    let finish = finish.ok_or_else(|| ended_early("finish_reason"))?;
    if !calls.is_empty() {
        message.insert("tool_calls".into(), json!(calls));
    }
    let mut response = json!({
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
    });
    if let Some(usage) = usage {
        response["usage"] = usage;
    }
    Ok(response)
}

fn add(message: &mut Object, calls: &mut Vec<Object>, delta: Object) -> Result<(), Failure> {
    for (key, value) in delta {
        match key.as_str() {
            "role" => given(message, key, value)?,
            "tool_calls" => match value {
                Value::Null => {}
                Value::Array(pieces) => {
                    for piece in pieces {
                        add_call(calls, piece)?;
                    }
                }
                _ => return Err(bad("tool_calls must be an array")),
            },
            _ => append(message, key, value),
        }
    }
    Ok(())
}

fn add_call(calls: &mut Vec<Object>, piece: Value) -> Result<(), Failure> {
    let Value::Object(mut piece) = piece else {
        return Err(bad("a tool call delta must be an object"));
    };
    let index = piece
        .remove("index")
        .and_then(|index| index.as_u64())
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| bad("a tool call delta must give its index"))?;
    if index == calls.len() {
        calls.push(Object::new());
    }
    let call = calls
        .get_mut(index)
        .ok_or_else(|| bad("tool call deltas must come in index order"))?;
    for (key, value) in piece {
        match (key.as_str(), value) {
            ("id" | "type", value) => given(call, key, value)?,
            ("function", Value::Object(function)) => {
                let Value::Object(slot) = call
                    .entry("function")
                    .or_insert_with(|| Value::Object(Object::new()))
                else {
                    return Err(bad("a tool call's function must be an object"));
                };
                for (key, value) in function {
                    if key == "name" {
                        given(slot, key, value)?;
                    } else {
                        append(slot, key, value);
                    }
                }
            }
            (_, value) => append(call, key, value),
        }
    }
    Ok(())
}

/// Set a field a stream gives once; a repeat must agree, and an empty or `null` one adds nothing.
fn given(slot: &mut Object, key: String, value: Value) -> Result<(), Failure> {
    if !carries(&value) || value.as_str() == Some("") {
        return Ok(());
    }
    match slot.get(&key) {
        Some(earlier) if *earlier != value => Err(bad(format!(
            "the stream gave {key} as {earlier} and then as {value}"
        ))),
        Some(_) => Ok(()),
        None => {
            slot.insert(key, value);
            Ok(())
        }
    }
}

/// Add a delta's `value` the way a stream spells a field across chunks.
///
/// Text appends, a list extends, `null` adds nothing, and anything else replaces what came before.
fn append(slot: &mut Object, key: String, value: Value) {
    match (slot.get_mut(&key), value) {
        (_, Value::Null) => {}
        (Some(Value::String(text)), Value::String(more)) => text.push_str(&more),
        (Some(Value::Array(items)), Value::Array(more)) => items.extend(more),
        (_, value) => {
            slot.insert(key, value);
        }
    }
}

/// The failure an `error` in a chunk reports.
fn reported(error: &Value) -> Failure {
    let status = error
        .get("code")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok());
    match status {
        Some(status) => status_failure(status, format!("the stream reported {error}")),
        None => bad(format!("the stream reported {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::samples::DEEPSEEK_CALL;
    use super::super::{decode, encode};
    use crate::protocol::{CompleteError, HttpResponse, Request, Settings, StopReason};
    use crate::types::{Failure, Message, Part, Role};
    use serde_json::{Value, json};

    fn streamed(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![(
                "content-type".into(),
                "text/event-stream; charset=utf-8".into(),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    fn failed(body: &str) -> Failure {
        match decode(&streamed(body)) {
            Err(CompleteError::Failed(failure)) => failure,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_stream_reads_as_the_reply_it_spells() -> Result<(), CompleteError> {
        let reply = decode(&streamed(DEEPSEEK_CALL))?;
        assert_eq!(reply.stop_reason, StopReason::ToolCalls);
        assert_eq!(reply.usage.input_tokens, 377);
        assert_eq!(reply.usage.output_tokens, 137);
        assert_eq!(reply.usage.cached_input_tokens, Some(0));
        let [
            Part::Text(text),
            Part::ToolCall(call),
            Part::Extension(whole),
        ] = reply.message.parts.as_slice()
        else {
            panic!("unexpected parts {:?}", reply.message.parts);
        };
        assert_eq!(text, "");
        assert_eq!(call.id, "call_00_bhaNxlLUZG1eZPHqNH9f9229");
        assert_eq!(call.name, "get_weather");
        assert_eq!(call.arguments, r#"{"city": "Nice"}"#);
        let whole: Value = serde_json::from_str(&whole.data).unwrap();
        assert!(
            whole["reasoning_content"]
                .as_str()
                .is_some_and(|text| text.starts_with("We need to compare weather"))
        );
        assert_eq!(
            whole["tool_calls"],
            json!([{"id": call.id, "type": "function", "function": {"name": "get_weather", "arguments": call.arguments}}])
        );
        Ok(())
    }

    #[test]
    fn a_stream_cut_before_its_finish_may_be_retried() {
        let cut = DEEPSEEK_CALL
            .split("\n\n")
            .filter(|event| !event.contains("finish_reason\":\"") && !event.contains("[DONE]"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let failure = failed(&cut);
        assert_eq!(failure.code, "provider.server");
        assert!(failure.retryable);
    }

    #[test]
    fn an_error_in_the_stream_fails_by_its_status() {
        let busy = failed("data: {\"error\":{\"code\":503,\"message\":\"busy\"}}\n\n");
        assert_eq!(busy.code, "provider.server");
        assert!(busy.retryable);
        let unknown = failed("data: {\"error\":{\"message\":\"odd\"}}\n\n");
        assert_eq!(unknown.code, "provider.bad_response");
    }

    #[test]
    fn requests_stream_with_usage_unless_the_owner_turns_it_off() -> Result<(), Failure> {
        let request = || Request {
            messages: vec![Message {
                role: Role::User,
                parts: vec![Part::Text("Hi.".into())],
            }],
            tools: vec![],
            max_output_tokens: 64,
        };
        let settings = |options: Value| Settings {
            model: "m".into(),
            options: options.to_string(),
        };
        let body = |request: &crate::protocol::HttpRequest| -> Value {
            serde_json::from_slice(request.body.as_deref().unwrap_or_default()).unwrap()
        };
        let streamed = encode(&settings(json!({"top_p": 1})), request())?;
        assert_eq!(body(&streamed)["stream"], true);
        assert_eq!(
            body(&streamed)["stream_options"],
            json!({"include_usage": true})
        );
        assert_eq!(body(&streamed)["top_p"], 1);
        assert!(streamed.idle_timeout_ms.is_some());
        let whole = encode(&settings(json!({"stream": false})), request())?;
        assert!(body(&whole).get("stream").is_none());
        assert!(body(&whole).get("stream_options").is_none());
        assert!(whole.idle_timeout_ms.is_none());
        Ok(())
    }
}
