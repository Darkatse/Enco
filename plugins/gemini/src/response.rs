use enco_sdk::protocol::{
    Completion, StopReason, Usage, bad,
    types::{Extension, Failure, Message, Part, Role, ToolCall},
};
use serde_json::{Map, Value};
use std::collections::HashSet;

type Object = Map<String, Value>;

pub fn completion(response: Value) -> Result<Completion, Failure> {
    let response = object(&response, "response")?;
    let Some(candidate) = read(response, "candidates", list)?.first() else {
        let feedback = read(response, "promptFeedback", message)?;
        return Err(bad(format!("no candidate: {}", Value::Object(feedback))));
    };
    let candidate = object(candidate, "candidate")?;
    let content = read(candidate, "content", message)?;
    let mut parts = canonical_parts(&content)?;
    let calls = parts.iter().any(|part| matches!(part, Part::ToolCall(_)));
    let stop_reason = match read(candidate, "finishReason", Value::as_str)? {
        "MAX_TOKENS" | "CONTINUATION" => StopReason::MaxTokens,
        "STOP" if calls => StopReason::ToolCalls,
        "STOP" => StopReason::EndTurn,
        _ => StopReason::Other,
    };
    parts.push(Part::Extension(Extension {
        data: Value::Object(content).to_string(),
    }));
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts,
        },
        usage: usage(response)?,
        stop_reason,
    })
}

fn canonical_parts(content: &Object) -> Result<Vec<Part>, Failure> {
    let mut parts = vec![];
    let mut ids = HashSet::new();
    for part in read(content, "parts", list)? {
        let part = object(part, "part")?;
        if let Some(call) = part.get("functionCall") {
            let call = object(call, "functionCall")?;
            let id = required(call, "id")?;
            if !ids.insert(id) {
                return Err(bad(format!("duplicate function call ID {id}")));
            }
            parts.push(Part::ToolCall(ToolCall {
                id: id.into(),
                name: required(call, "name")?.into(),
                arguments: Value::Object(read(call, "args", message)?).to_string(),
            }));
        } else if let Some(text) = part.get("text") {
            let text = text.as_str().ok_or_else(|| bad("invalid text"))?;
            if !read(part, "thought", Value::as_bool)? {
                parts.push(Part::Text(text.into()));
            }
        }
    }
    Ok(parts)
}

fn usage(response: &Object) -> Result<Usage, Failure> {
    let metadata = read(response, "usageMetadata", message)?;
    let tokens = |name| read(&metadata, name, Value::as_u64);
    Ok(Usage {
        input_tokens: tokens("promptTokenCount")?,
        // Thinking is billed as output.
        output_tokens: tokens("candidatesTokenCount")? + tokens("thoughtsTokenCount")?,
        cached_input_tokens: Some(tokens("cachedContentTokenCount")?),
    })
}

pub fn embeddings(response: Value, inputs: usize) -> Result<Vec<Vec<f32>>, Failure> {
    let embeddings = read(object(&response, "response")?, "embeddings", list)?;
    if embeddings.len() != inputs {
        return Err(bad("embedding count does not match input count"));
    }
    embeddings
        .iter()
        .map(|embedding| {
            read(object(embedding, "embedding")?, "values", list)?
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .map(|value| value as f32)
                        .ok_or_else(|| bad("invalid embedding value"))
                })
                .collect()
        })
        .collect()
}

/// Reads a field as proto3 JSON writes it: an absent field holds its type's default (an
/// empty list or message, an unspecified enum, zero, false), so only a present field of
/// the wrong type makes a response malformed.
fn read<'a, T: Default>(
    value: &'a Object,
    name: &str,
    typed: fn(&'a Value) -> Option<T>,
) -> Result<T, Failure> {
    match value.get(name) {
        None => Ok(T::default()),
        Some(field) => typed(field).ok_or_else(|| bad(format!("invalid {name}"))),
    }
}

fn list(value: &Value) -> Option<&[Value]> {
    value.as_array().map(Vec::as_slice)
}

fn message(value: &Value) -> Option<Object> {
    value.as_object().cloned()
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Object, Failure> {
    value
        .as_object()
        .ok_or_else(|| bad(format!("invalid {what}")))
}

/// A field Enco cannot do without, such as a call's ID, which this plugin never invents.
fn required<'a>(value: &'a Object, name: &str) -> Result<&'a str, Failure> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("missing string {name}")))
}
