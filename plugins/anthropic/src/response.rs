//! The canonical completion for a Messages response: text and tool calls, with the whole reply
//! kept in an extension whenever its content holds more than they can say.
use enco_sdk::protocol::{
    Completion, StopReason, Usage, bad, carries, extras,
    types::{Extension, Failure, Message, Part, Role, ToolCall},
};
use serde_json::{Value, json};
use std::collections::HashSet;

/// The completion a successful Messages response states.
///
/// A content block of a type the contract has no part for, such as thinking, or a block with
/// fields beyond its canonical ones, such as a call's `caller`, keeps the whole reply in one
/// extension, which `encode` sends back as it came.
pub fn completion(response: &Value) -> Result<Completion, Failure> {
    if response.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err(bad("message role must be assistant"));
    }
    let content = response
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("missing content"))?;
    let mut parts = vec![];
    let mut ids = HashSet::new();
    let mut carried = false;
    for block in content {
        let Some((part, canonical)) = canonical(block)? else {
            carried = true;
            continue;
        };
        if let Part::ToolCall(call) = &part
            && !ids.insert(call.id.clone())
        {
            return Err(bad(format!("duplicate tool_use ID {}", call.id)));
        }
        carried |= carries(&Value::Object(extras(block.clone(), &canonical)));
        parts.push(part);
    }
    if carried {
        let whole = json!({"role": "assistant", "content": content});
        parts.push(Part::Extension(Extension {
            data: whole.to_string(),
        }));
    }
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts,
        },
        usage: usage(response)?,
        stop_reason: stop_reason(response)?,
    })
}

/// The canonical part of a content block and the block re-encoded from that part alone, or none
/// for a block of a type the contract has no part for.
fn canonical(block: &Value) -> Result<Option<(Part, Value)>, Failure> {
    Ok(match string(block, "type")? {
        "text" => {
            let text = string(block, "text")?;
            let canonical = json!({"type": "text", "text": text});
            Some((Part::Text(text.into()), canonical))
        }
        "tool_use" => {
            let (id, name) = (string(block, "id")?, string(block, "name")?);
            let input = block
                .get("input")
                .filter(|input| input.is_object())
                .ok_or_else(|| bad("tool_use input must be an object"))?;
            let call = ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: input.to_string(),
            };
            let canonical = json!({"type": "tool_use", "id": id, "name": name, "input": input});
            Some((Part::ToolCall(call), canonical))
        }
        _ => None,
    })
}

fn usage(response: &Value) -> Result<Usage, Failure> {
    let usage = response.get("usage").ok_or_else(|| bad("missing usage"))?;
    let count = |name: &str| {
        usage
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| bad(format!("missing {name}")))
    };
    // The protocol types the cache counts as nullable: unreported.
    let cache = |name: &str| match usage.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => count(name).map(Some),
    };
    let read = cache("cache_read_input_tokens")?;
    Ok(Usage {
        // `input_tokens` is only what the cache neither wrote nor served.
        input_tokens: count("input_tokens")?
            + cache("cache_creation_input_tokens")?.unwrap_or(0)
            + read.unwrap_or(0),
        output_tokens: count("output_tokens")?,
        cached_input_tokens: read,
    })
}

fn stop_reason(response: &Value) -> Result<StopReason, Failure> {
    Ok(match string(response, "stop_reason")? {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolCalls,
        "max_tokens" | "model_context_window_exceeded" => StopReason::MaxTokens,
        _ => StopReason::Other,
    })
}

fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, Failure> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("missing string {name}")))
}
