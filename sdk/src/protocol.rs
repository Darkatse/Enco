//! Helpers for plugins that call model services through the host's `http` import, including
//! OpenAI-compatible Chat Completions and embeddings.
use crate::bindings;
pub use crate::http::{PostError, apply_options, post};
pub use bindings::enco::plugin::{host, types};
pub use bindings::exports::enco::plugin::completion::{
    CompleteError, Completion, Request, StopReason,
};
use serde_json::{Map, Value, json};
use types::{Failure, Message, Part, Role};
pub use types::{Overflow, Settings, Usage};
mod messages;

/// A failure with the given code; `retryable` means the same request may succeed later.
pub fn failure(code: &str, message: impl Into<String>, retryable: bool) -> Failure {
    Failure {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

/// A non-retryable `provider.bad_response` failure: the reply is malformed or not what the protocol
/// promises.
pub fn bad(message: impl Into<String>) -> Failure {
    failure("provider.bad_response", message, false)
}

/// A non-retryable `provider.bad_request` failure: the request cannot succeed as written.
pub fn bad_request(message: impl Into<String>) -> Failure {
    failure("provider.bad_request", message, false)
}

/// Classify a refused completion request.
///
/// `read` is the protocol's reading of the service's JSON answer: an overflow it recognizes, with
/// the numbers the service gave. An HTTP 413 is an overflow even when `read` recognizes nothing,
/// since the status itself says the request is too large. Anything else stays the failure
/// [`post`] classified.
pub fn complete_error(
    error: PostError,
    read: impl FnOnce(&Value) -> Option<Overflow>,
) -> CompleteError {
    let PostError {
        status,
        body,
        failure,
    } = error;
    if let Some(overflow) = serde_json::from_str::<Value>(&body)
        .ok()
        .as_ref()
        .and_then(read)
    {
        return CompleteError::Overflow(overflow);
    }
    if status == Some(413) {
        return CompleteError::Overflow(Overflow {
            counted: None,
            accepted: None,
            message: failure.message,
        });
    }
    CompleteError::Failed(failure)
}

/// The whole number that follows `marker` in a service's message, such as a token limit it
/// states in prose.
pub fn number_after(message: &str, marker: &str) -> Option<u64> {
    let rest = &message[message.find(marker)? + marker.len()..];
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..digits].parse().ok()
}

/// Accept only `{}` as the plugin's configuration; anything else fails with `config.invalid`.
///
/// Use it in plugins whose endpoint, model and options all arrive with each call's [`Settings`].
pub fn empty_config(config: &str) -> Result<(), Failure> {
    let value: Value = serde_json::from_str(config)
        .map_err(|e| failure("config.invalid", e.to_string(), false))?;
    if value.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err(failure(
            "config.invalid",
            "expected an empty configuration object",
            false,
        ))
    }
}

/// Send `request` as one non-streaming Chat Completions call and return the reply as a
/// [`Completion`].
///
/// `settings.options` may add fields to the request, but not replace one this function writes or
/// reserves, such as `model`, `messages` or `stream`. A reply with fields the contract has no
/// place for is kept whole in an extension part and sent back as it came. A refusal that states
/// the model's maximum context length, as DeepSeek's does, is an overflow.
pub async fn complete(settings: Settings, request: Request) -> Result<Completion, CompleteError> {
    let body = chat_request(&settings, request).map_err(CompleteError::Failed)?;
    let response = post(&settings, "chat/completions", body, None)
        .await
        .map_err(|error| complete_error(error, chat_overflow))?;
    parse_completion(response).map_err(CompleteError::Failed)
}

/// A Chat Completions refusal of an over-long request, in the words DeepSeek uses.
///
/// "This model's maximum context length is W tokens. However, you requested T tokens (M in the
/// messages, C in the completion)." The window counts the completion, so the input it accepts is
/// W − C. Only that breakdown gives numbers; a refusal that splits the request otherwise, or not
/// at all, is an overflow without them.
fn chat_overflow(answer: &Value) -> Option<Overflow> {
    let message = answer.pointer("/error/message")?.as_str()?;
    let window = number_after(message, "maximum context length is ")?;
    let breakdown = message.split_once(" tokens (").and_then(|(_, requested)| {
        let (messages, rest) = requested.split_once(" in the messages, ")?;
        let (completion, _) = rest.split_once(" in the completion)")?;
        Some((messages.parse().ok()?, completion.parse::<u64>().ok()?))
    });
    Some(Overflow {
        counted: breakdown.map(|(messages, _)| messages),
        accepted: breakdown.map(|(_, completion)| window.saturating_sub(completion)),
        message: message.into(),
    })
}

fn chat_request(settings: &Settings, request: Request) -> Result<Value, Failure> {
    let messages = messages::encode(&request.messages)?;
    let mut body = json!({ "model": settings.model, "messages": messages });
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                let parameters = serde_json::from_str::<Value>(&tool.input_schema)
                    .map_err(|e| bad_request(e.to_string()))?;
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": parameters,
                    },
                }))
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        body["tools"] = json!(tools);
    }
    if let Some(max) = request.max_output_tokens {
        body["max_tokens"] = json!(max);
    }
    apply_options(
        &mut body,
        &settings.options,
        &[
            "model",
            "messages",
            "tools",
            "max_tokens",
            "max_completion_tokens",
            "stream",
            "stream_options",
        ],
    )?;
    Ok(body)
}

/// Embed each input through the `embeddings` endpoint, returning one vector per input, in input
/// order.
///
/// The request times out after 30 seconds. The reply must hold exactly one non-empty vector of
/// finite numbers for each input, all of the same length; otherwise the call fails as
/// `provider.bad_response`.
pub async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
    let count = inputs.len();
    let mut body = json!({ "model": settings.model, "input": inputs });
    apply_options(
        &mut body,
        &settings.options,
        &["model", "input", "encoding_format"],
    )?;
    let response = post(&settings, "embeddings", body, Some(30000)).await?;
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("missing embeddings data"))?;
    if data.len() != count {
        return Err(bad("embedding count does not match input count"));
    }
    let mut embeddings: Vec<_> = data
        .iter()
        .map(|embedding| {
            // Compatible APIs may omit a scalar's default zero value.
            // Coverage validation below still rejects duplicate or missing positions.
            let index = match embedding.get("index") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| bad("invalid embedding index"))?,
            };
            let vector: Vec<f32> = serde_json::from_value(
                embedding
                    .get("embedding")
                    .cloned()
                    .ok_or_else(|| bad("missing embedding"))?,
            )
            .map_err(|e| bad(e.to_string()))?;
            if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
                return Err(bad("embedding must contain finite numbers"));
            }
            Ok((index, vector))
        })
        .collect::<Result<_, Failure>>()?;
    if embeddings
        .windows(2)
        .any(|pair| pair[0].1.len() != pair[1].1.len())
    {
        return Err(bad("embedding vectors have inconsistent widths"));
    }
    embeddings.sort_by_key(|(index, _)| *index);
    if embeddings
        .iter()
        .enumerate()
        .any(|(i, (index, _))| *index != i as u64)
    {
        return Err(bad("embedding indexes must cover the input once"));
    }
    Ok(embeddings.into_iter().map(|(_, vector)| vector).collect())
}

fn wire_call(call: &types::ToolCall) -> Value {
    json!({
        "id": call.id,
        "type": "function",
        "function": { "name": call.name, "arguments": call.arguments },
    })
}

/// What `wire` carries beyond `canonical`, in the shape of `wire`.
fn extras(wire: Value, canonical: &Value) -> Map<String, Value> {
    let (Value::Object(mut wire), Value::Object(canonical)) = (wire, canonical) else {
        return Map::new();
    };
    for (key, canonical) in canonical {
        if let Some(value) = wire.remove(key) {
            let rest = extras(value, canonical);
            if !rest.is_empty() {
                wire.insert(key.clone(), Value::Object(rest));
            }
        }
    }
    wire
}

/// Whether service fields carry anything: in JSON, `null` and an empty list or object mean the
/// same as an absent field.
fn carries(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => fields.values().any(carries),
        _ => true,
    }
}

fn parse_completion(response: Value) -> Result<Completion, Failure> {
    let choice = response
        .pointer("/choices/0")
        .ok_or_else(|| bad("missing choices[0]"))?;
    let message = parse_message(choice.get("message"))?;
    let usage = parse_usage(response.get("usage"))?;
    let stop_reason = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") => StopReason::ToolCalls,
        Some("length") => StopReason::MaxTokens,
        Some(_) => StopReason::Other,
        // A response cut off upstream, for example by moderation, may not say how it ended.
        None => return Err(bad("finish_reason must be a string")),
    };
    Ok(Completion {
        message,
        usage,
        stop_reason,
    })
}

fn parse_message(value: Option<&Value>) -> Result<Message, Failure> {
    let whole = value
        .and_then(Value::as_object)
        .ok_or_else(|| bad("missing assistant message"))?;
    let mut wire = whole.clone();
    match wire.remove("role") {
        Some(Value::String(role)) if role == "assistant" => {}
        _ => return Err(bad("completion role must be assistant")),
    }
    let mut parts = vec![];
    match wire.remove("content") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => parts.push(Part::Text(text)),
        Some(_) => return Err(bad("completion content must be text or null")),
    }
    let calls = match wire.remove("tool_calls") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(calls)) => calls,
        Some(_) => return Err(bad("tool_calls must be an array")),
    };
    let mut ids = std::collections::HashSet::new();
    let mut carried = carries(&Value::Object(wire));
    for call in calls {
        let parsed = types::ToolCall {
            id: field(&call, "/id")?,
            name: field(&call, "/function/name")?,
            arguments: field(&call, "/function/arguments")?,
        };
        if !ids.insert(parsed.id.clone()) {
            return Err(bad("duplicate provider call ID"));
        }
        carried |= carries(&Value::Object(extras(call, &wire_call(&parsed))));
        parts.push(Part::ToolCall(parsed));
    }
    // A reply with more than the canonical fields goes back whole, as the service wrote it.
    if carried {
        parts.push(Part::Extension(types::Extension {
            data: Value::Object(whole.clone()).to_string(),
        }));
    }
    Ok(Message {
        role: Role::Assistant,
        parts,
    })
}

fn parse_usage(value: Option<&Value>) -> Result<Usage, Failure> {
    let usage = value.ok_or_else(|| bad("missing usage"))?;
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("missing prompt_tokens"))?;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("missing completion_tokens"))?;
    let cached_input_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64);
    Ok(Usage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
    })
}

fn field(value: &Value, path: &str) -> Result<String, Failure> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| bad(format!("missing text at {path}")))
}
