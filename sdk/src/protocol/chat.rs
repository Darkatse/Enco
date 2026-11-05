//! OpenAI-compatible Chat Completions and embeddings, as the pure encode and decode a Provider
//! exports.
use super::{
    CompleteError, Completion, EMBEDDING_TIMEOUT_MS, HttpRequest, HttpResponse, Overflow, Request,
    Settings, StopReason, Usage, answer, apply_options, bad, bad_request, carries, embeddings,
    extras, json, number_after, post_json,
    stream::{post_completion, streaming},
};
use crate::types::{self, Failure, Message, Part, Role};
use serde_json::{Value, json};
mod chunks;
mod messages;
#[cfg(test)]
mod samples;

/// The request for one Chat Completions call, streamed with its usage unless the options set
/// `stream` to `false`.
///
/// `settings.options` may add fields to the request, but not replace one this function writes or
/// reserves, such as `model`, `messages` or `stream_options`.
pub fn encode(settings: &Settings, request: Request) -> Result<HttpRequest, Failure> {
    let (stream, options) = streaming(&settings.options)?;
    let messages = messages::encode(&request.messages)?;
    let mut body = json!({
        "model": settings.model,
        "messages": messages,
        "max_tokens": request.max_output_tokens,
    });
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
    if stream {
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});
    }
    apply_options(
        &mut body,
        &options,
        &[
            "model",
            "messages",
            "tools",
            "max_tokens",
            "max_completion_tokens",
            "stream_options",
        ],
    )?;
    Ok(post_completion("/chat/completions", &body, stream))
}

/// What a Chat Completions response means, streamed or not.
///
/// A stream folds into the response it is in one piece (see `chunks::fold`). A reply with fields
/// the contract has no place for is kept whole in an extension part and sent back as it came. A
/// refusal that states the model's maximum context length, as DeepSeek's does, is an overflow.
/// A service that streams no usage fails with `missing usage`; the owner can then turn streaming
/// off.
pub fn decode(response: &HttpResponse) -> Result<Completion, CompleteError> {
    let response = answer(response, overflow, chunks::fold)?;
    parse_completion(response).map_err(CompleteError::Failed)
}

/// A Chat Completions refusal of an over-long request, in the words DeepSeek uses.
///
/// "This model's maximum context length is W tokens. However, you requested T tokens (M in the
/// messages, C in the completion)." The window counts the completion, so the input it accepts is
/// W − C. Only that breakdown gives numbers; a refusal that splits the request otherwise, or not
/// at all, is an overflow without them.
fn overflow(answer: &Value) -> Option<Overflow> {
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

/// The request that embeds each input through the `embeddings` endpoint.
///
/// It times out after 30 seconds. `settings.options` may add fields, but not replace `model`,
/// `input` or `encoding_format`.
pub fn encode_embedding(settings: &Settings, inputs: &[String]) -> Result<HttpRequest, Failure> {
    let mut body = json!({ "model": settings.model, "input": inputs });
    apply_options(
        &mut body,
        &settings.options,
        &["model", "input", "encoding_format"],
    )?;
    Ok(post_json("/embeddings", &body, Some(EMBEDDING_TIMEOUT_MS)))
}

/// One vector per input, in input order, from an `embeddings` response to `count` inputs.
///
/// The indexes must cover the vectors once, and the vectors must be the embeddings
/// [`embeddings`] accepts; otherwise the response is `provider.bad_response`.
pub fn decode_embedding(count: u32, response: &HttpResponse) -> Result<Vec<Vec<f32>>, Failure> {
    let response = json(response)?;
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("missing embeddings data"))?;
    let mut indexed: Vec<_> = data
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
            Ok((index, vector))
        })
        .collect::<Result<_, Failure>>()?;
    indexed.sort_by_key(|(index, _)| *index);
    if indexed
        .iter()
        .enumerate()
        .any(|(i, (index, _))| *index != i as u64)
    {
        return Err(bad("embedding indexes must cover the input once"));
    }
    embeddings(
        count,
        indexed.into_iter().map(|(_, vector)| vector).collect(),
    )
}

/// A call as Chat Completions writes it, under `id`.
fn wire_call(id: &str, call: &types::ToolCall) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": { "name": call.name, "arguments": call.arguments },
    })
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
        carried |= carries(&Value::Object(extras(
            call,
            &wire_call(&parsed.id, &parsed),
        )));
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
    let usage = value.ok_or_else(|| {
        bad("missing usage; a service that streams none needs the option \"stream\": false")
    })?;
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
