//! The canonical completion for a Responses response: text and function calls, with the whole
//! `output` kept in an extension whenever its items hold more than they can say.
use enco_sdk::protocol::{
    Completion, StopReason, Usage, bad, carries, extras,
    types::{Extension, Failure, Message, Part, Role, ToolCall},
};
use serde_json::{Value, json};
use std::collections::HashSet;

/// The completion a successful Responses response states.
///
/// An output item of a type the contract has no part for, such as reasoning, or an item with
/// fields beyond its canonical ones, such as `id` and `status`, keeps the whole `output` in one
/// extension, which `encode` sends back as it came. Real items always carry such fields, so in
/// practice every reply keeps one. A `failed` response is `provider.bad_response` with the
/// service's message.
pub fn completion(response: &Value) -> Result<Completion, Failure> {
    let status = string(response, "status")?;
    if status == "failed" {
        let message = response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a failed response without error.message"))?;
        return Err(bad(format!("the response failed: {message}")));
    }
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("missing output"))?;
    let mut parts = vec![];
    let mut ids = HashSet::new();
    let mut carried = false;
    for item in output {
        let (item_parts, beyond) = canonical(item)?;
        carried |= beyond;
        for part in item_parts {
            if let Part::ToolCall(call) = &part
                && !ids.insert(call.id.clone())
            {
                return Err(bad(format!("duplicate call_id {}", call.id)));
            }
            parts.push(part);
        }
    }
    let called = !ids.is_empty();
    if carried {
        parts.push(Part::Extension(Extension {
            data: Value::Array(output.clone()).to_string(),
        }));
    }
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts,
        },
        usage: usage(response)?,
        stop_reason: stop_reason(response, status, called)?,
    })
}

/// The canonical parts of an output item, and whether the item carries more than they say: an
/// item of a type the contract has no part for, or fields beyond the item re-encoded from them.
fn canonical(item: &Value) -> Result<(Vec<Part>, bool), Failure> {
    Ok(match string(item, "type")? {
        "message" => message(item)?,
        "function_call" => {
            let call = ToolCall {
                id: string(item, "call_id")?.into(),
                name: string(item, "name")?.into(),
                arguments: string(item, "arguments")?.into(),
            };
            let canonical = json!({
                "type": "function_call",
                "call_id": call.id,
                "name": call.name,
                "arguments": call.arguments,
            });
            (vec![Part::ToolCall(call)], beyond(item, &canonical))
        }
        _ => (vec![], true),
    })
}

/// The texts of a message item, and whether it carries more than they say.
///
/// `extras` does not look into arrays, so each content entry is weighed on its own; an entry other
/// than `output_text`, such as a refusal, has no canonical part.
fn message(item: &Value) -> Result<(Vec<Part>, bool), Failure> {
    if string(item, "role")? != "assistant" {
        return Err(bad("message role must be assistant"));
    }
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("message content must be an array"))?;
    let shell = json!({"type": "message", "role": "assistant", "content": []});
    let mut carried = beyond(item, &shell);
    let mut parts = vec![];
    for entry in content {
        if string(entry, "type")? == "output_text" {
            let text = string(entry, "text")?;
            carried |= beyond(entry, &json!({"type": "output_text", "text": text}));
            parts.push(Part::Text(text.into()));
        } else {
            carried = true;
        }
    }
    Ok((parts, carried))
}

/// Whether `wire` carries anything beyond `canonical`, as [`extras`] and [`carries`] weigh it.
fn beyond(wire: &Value, canonical: &Value) -> bool {
    carries(&Value::Object(extras(wire.clone(), canonical)))
}

fn usage(response: &Value) -> Result<Usage, Failure> {
    let usage = response.get("usage").ok_or_else(|| bad("missing usage"))?;
    let count = |name: &str| {
        usage
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| bad(format!("missing {name}")))
    };
    // A cache count the service leaves out or nulls is one it does not report.
    let cached = match usage.pointer("/input_tokens_details/cached_tokens") {
        None | Some(Value::Null) => None,
        Some(cached) => Some(
            cached
                .as_u64()
                .ok_or_else(|| bad("cached_tokens must be a count"))?,
        ),
    };
    Ok(Usage {
        // `input_tokens` already includes what the cache served.
        input_tokens: count("input_tokens")?,
        output_tokens: count("output_tokens")?,
        cached_input_tokens: cached,
    })
}

/// Why generation stopped. A request that is neither background nor streamed ends `completed`,
/// `incomplete` or `failed`, so any other status is `provider.bad_response`.
fn stop_reason(response: &Value, status: &str, called: bool) -> Result<StopReason, Failure> {
    Ok(match status {
        "completed" if called => StopReason::ToolCalls,
        "completed" => StopReason::EndTurn,
        "incomplete" => match response
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
        {
            Some("max_output_tokens") => StopReason::MaxTokens,
            _ => StopReason::Other,
        },
        _ => return Err(bad(format!("unexpected status {status}"))),
    })
}

fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, Failure> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("missing string {name}")))
}
