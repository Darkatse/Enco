//! Pure helpers for Provider and decision mappers: the request a service needs, what its response
//! means, and the owner's options merged into the request.
//!
//! OpenAI-compatible Chat Completions and embeddings are in [`chat`], and what streamed
//! completions share is in [`stream`]. A mapper imports nothing: it returns the request, and the
//! host sends it to the endpoint and hands the whole response back, whatever its status.
use crate::bindings;
pub use bindings::enco::plugin::types;
pub use bindings::exports::enco::plugin::completion::{
    CompleteError, Completion, Request, StopReason,
};
use serde_json::{Map, Value};
use types::{Failure, Message, Part, Role, ToolCall, ToolResult};
pub use types::{HttpRequest, HttpResponse, Overflow, Settings, Usage};
pub mod chat;
pub mod stream;
use stream::{Event, status_failure};

/// How long a completion request that is not streamed may take, in milliseconds.
///
/// Ten minutes is the default of Anthropic's and OpenAI's own clients: such a reply arrives only
/// once it is complete, and a long one takes minutes.
pub const COMPLETION_TIMEOUT_MS: u32 = 600_000;

/// How long an embedding request may take, in milliseconds.
pub const EMBEDDING_TIMEOUT_MS: u32 = 30_000;

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

/// A POST of `body` as JSON to `url`.
///
/// A mapper gives a path starting with `/`, which the host joins to the endpoint. `None` for
/// `timeout_ms` means the host's default.
pub fn post_json(url: impl Into<String>, body: &Value, timeout_ms: Option<u32>) -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        url: url.into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some(body.to_string().into_bytes()),
        timeout_ms,
        idle_timeout_ms: None,
    }
}

/// The JSON of a successful response, or the failure its status means ([`status_failure`]).
///
/// A successful body that is not JSON is `provider.bad_response`.
pub fn json(response: &HttpResponse) -> Result<Value, Failure> {
    if !(200..300).contains(&response.status) {
        return Err(refusal(response));
    }
    serde_json::from_slice(&response.body).map_err(|e| bad(format!("invalid JSON response: {e}")))
}

/// What a completion response means: the JSON of its reply, an overflow, or a failure.
///
/// A successful response in the `text/event-stream` media type is a stream: [`stream::events`]
/// reads it and `fold` turns its events into the JSON the same reply has as one response, or into
/// the error the stream reported. Any other successful response is that JSON itself. So a stream,
/// a reply sent whole and a response recorded before streaming read the same way.
///
/// A refusal is the failure [`json`] describes, unless the service turned the request away as too
/// large. `read` is the protocol's reading of a refusal's JSON: an overflow it recognizes, with
/// the numbers the service gave. An HTTP 413 is an overflow even when `read` recognizes nothing,
/// since the status itself says the request is too large.
pub fn answer(
    response: &HttpResponse,
    read: impl FnOnce(&Value) -> Option<Overflow>,
    fold: impl FnOnce(Vec<Event>) -> Result<Value, CompleteError>,
) -> Result<Value, CompleteError> {
    if (200..300).contains(&response.status) {
        if streamed(response) {
            return fold(stream::events(&response.body).map_err(CompleteError::Failed)?);
        }
        return json(response).map_err(CompleteError::Failed);
    }
    if let Some(overflow) = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .as_ref()
        .and_then(read)
    {
        return Err(CompleteError::Overflow(overflow));
    }
    let failure = refusal(response);
    if response.status == 413 {
        return Err(CompleteError::Overflow(Overflow {
            counted: None,
            accepted: None,
            message: failure.message,
        }));
    }
    Err(CompleteError::Failed(failure))
}

/// The failure a response's status means, quoting the start of its body.
fn refusal(response: &HttpResponse) -> Failure {
    let mut excerpt = String::from_utf8_lossy(&response.body).into_owned();
    excerpt.truncate(excerpt.floor_char_boundary(500));
    status_failure(
        response.status,
        format!("HTTP {}: {excerpt}", response.status),
    )
}

/// Whether the response's media type is `text/event-stream`.
fn streamed(response: &HttpResponse) -> bool {
    response.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type")
            && value
                .split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
    })
}

/// `vectors` as the embeddings of `count` inputs, checked against the protocol's promise.
///
/// They are one nonempty vector of finite numbers per input, all of one width; otherwise the
/// service's reply is `provider.bad_response`.
pub fn embeddings(count: u32, vectors: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>, Failure> {
    if usize::try_from(count) != Ok(vectors.len()) {
        return Err(bad(format!(
            "{} embeddings for {count} inputs",
            vectors.len()
        )));
    }
    let width = vectors.first().map_or(0, Vec::len);
    if vectors.iter().any(|vector| {
        vector.is_empty() || vector.len() != width || vector.iter().any(|value| !value.is_finite())
    }) {
        return Err(bad("embeddings must be nonempty, finite and of one width"));
    }
    Ok(vectors)
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

/// A request's messages grouped as a protocol sends them.
///
/// System and user messages stand alone, and each assistant message comes with the tool messages
/// after it, which answer its calls.
pub enum Turn<'a> {
    /// A system or user message.
    Message(&'a Message),
    /// An assistant message at `position` in the request, and each of its calls in order.
    Assistant {
        /// The assistant message.
        message: &'a Message,
        /// Its index among the request's messages, from which [`call_id`] names its calls.
        position: usize,
        /// Its calls in order, each with the result that answers it.
        calls: Vec<Call<'a>>,
    },
}

/// A call, the ID it goes out under, and the tool message that answers it.
pub struct Call<'a> {
    /// The ID the call and its result share.
    ///
    /// A message with an extension goes out whole, as its plugin stored it, so its calls keep the
    /// service's IDs, even empty ones; a call rebuilt from canonical parts takes [`call_id`].
    pub id: String,
    /// The call as the assistant message holds it, whose own `id` may be empty.
    pub call: &'a ToolCall,
    /// The tool message that answers it: its result, then any images.
    pub message: &'a Message,
    /// The result that message starts with.
    pub result: &'a ToolResult,
}

/// The turns of `messages`.
///
/// A call and its result pair by the service's ID, or in order among the calls it gave none. The
/// host sends a tool message only right after the assistant message whose call it answers, with
/// a result for every call; a request that breaks this is `provider.bad_request`.
pub fn turns(messages: &[Message]) -> Result<Vec<Turn<'_>>, Failure> {
    let mut turns = vec![];
    let mut index = 0;
    while let Some(message) = messages.get(index) {
        let position = index;
        index += 1;
        match message.role {
            Role::Tool => {
                return Err(bad_request(
                    "a tool message must follow the assistant message whose call it answers",
                ));
            }
            Role::Assistant => {
                let answers = messages[index..]
                    .iter()
                    .take_while(|next| next.role == Role::Tool)
                    .count();
                let calls = answered(message, position, &messages[index..index + answers])?;
                index += answers;
                turns.push(Turn::Assistant {
                    message,
                    position,
                    calls,
                });
            }
            Role::System | Role::User => turns.push(Turn::Message(message)),
        }
    }
    Ok(turns)
}

/// The ID of the `index`th call of the assistant message at `position` when rebuilt from parts.
///
/// It is the service's own, or `call_{position}_{index}` when it gave none.
pub fn call_id(call: &ToolCall, position: usize, index: usize) -> String {
    if call.id.is_empty() {
        format!("call_{position}_{index}")
    } else {
        call.id.clone()
    }
}

fn answered<'a>(
    assistant: &'a Message,
    position: usize,
    tools: &'a [Message],
) -> Result<Vec<Call<'a>>, Failure> {
    let results: Vec<_> = tools
        .iter()
        .filter_map(|message| match message.parts.first() {
            Some(Part::ToolResult(result)) => Some((message, result)),
            _ => None,
        })
        .collect();
    let mut unnamed = results
        .iter()
        .filter(|(_, result)| result.call_id.is_empty());
    let whole = assistant
        .parts
        .iter()
        .any(|part| matches!(part, Part::Extension(_)));
    assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
        .enumerate()
        .map(|(index, call)| {
            let found = if call.id.is_empty() {
                unnamed.next()
            } else {
                results.iter().find(|(_, result)| result.call_id == call.id)
            };
            let (message, result) =
                found.ok_or_else(|| bad_request(format!("missing result for {}", call.name)))?;
            Ok(Call {
                id: if whole {
                    call.id.clone()
                } else {
                    call_id(call, position, index)
                },
                call,
                message,
                result,
            })
        })
        .collect()
}

/// What `wire` carries beyond `canonical`, in the shape of `wire`: the fields `canonical` lacks,
/// and below the fields both have, what differs there.
pub fn extras(wire: Value, canonical: &Value) -> Map<String, Value> {
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
pub fn carries(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => fields.values().any(carries),
        _ => true,
    }
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
