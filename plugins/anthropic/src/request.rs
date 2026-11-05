//! The Messages request body for a canonical completion request: system text at the top level,
//! each batch of tool results in one user message, an ID on every call, and the cache marker.
use enco_sdk::protocol::{
    Call, Request, Settings, Turn, apply_options, bad_request, call_id, turns,
    types::{Failure, Image, Message, Part, Role, ToolResult, ToolSpec},
};
use serde_json::{Value, json};

/// The fields this plugin writes or must leave out, which the owner's options cannot set.
const RESERVED: &[&str] = &[
    "model",
    "messages",
    "system",
    "tools",
    "max_tokens",
    "stream",
    "cache_control",
];

/// The body of one non-streaming Messages request.
///
/// Roles stay as the history has them: the service accepts a history that starts with the
/// assistant and repeated roles.
pub fn completion(settings: &Settings, request: Request) -> Result<Value, Failure> {
    let mut system = vec![];
    let mut messages = vec![];
    for turn in turns(&request.messages)? {
        match turn {
            Turn::Message(message) if message.role == Role::System => {
                system.extend(message.parts.iter().filter_map(|part| match part {
                    Part::Text(text) => text_block(text),
                    _ => None,
                }));
            }
            Turn::Message(message) => {
                messages.push(json!({"role": "user", "content": user(message)?}));
            }
            Turn::Assistant {
                message,
                position,
                calls,
            } => {
                messages.push(assistant(message, position)?);
                if !calls.is_empty() {
                    messages.push(results(&calls));
                }
            }
        }
    }
    let mut body = json!({
        "model": settings.model,
        "max_tokens": request.max_output_tokens,
        // Automatic caching: without this marker the service caches nothing.
        "cache_control": {"type": "ephemeral"},
        "messages": messages,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .into_iter()
            .map(tool)
            .collect::<Result<Vec<_>, _>>()?;
        body["tools"] = json!(tools);
    }
    apply_options(&mut body, &settings.options, RESERVED)?;
    Ok(body)
}

fn tool(tool: ToolSpec) -> Result<Value, Failure> {
    let schema: Value =
        serde_json::from_str(&tool.input_schema).map_err(|error| bad_request(error.to_string()))?;
    Ok(json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": schema,
    }))
}

fn user(message: &Message) -> Result<Vec<Value>, Failure> {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => text_block(text).map(Ok),
            Part::Image(image) => Some(Ok(image_block(image))),
            _ => Some(Err(bad_request(
                "a user message holds only text and images",
            ))),
        })
        .collect()
}

/// The assistant message at `position`: the reply this plugin kept whole, exactly as stored, or
/// one built from the canonical parts.
fn assistant(message: &Message, position: usize) -> Result<Value, Failure> {
    if let Some(data) = message.parts.iter().find_map(|part| match part {
        Part::Extension(extension) => Some(&extension.data),
        _ => None,
    }) {
        return serde_json::from_str(data).map_err(|error| bad_request(error.to_string()));
    }
    let mut content = vec![];
    let mut calls = 0;
    for part in &message.parts {
        match part {
            Part::Text(text) => content.extend(text_block(text)),
            Part::ToolCall(call) => {
                let input: Value = serde_json::from_str(&call.arguments)
                    .map_err(|error| bad_request(error.to_string()))?;
                if !input.is_object() {
                    return Err(bad_request(format!(
                        "the arguments of {} must be a JSON object",
                        call.name
                    )));
                }
                let id = call_id(call, position, calls);
                content
                    .push(json!({"type": "tool_use", "id": id, "name": call.name, "input": input}));
                calls += 1;
            }
            _ => {
                return Err(bad_request(
                    "an assistant message holds only text and tool calls",
                ));
            }
        }
    }
    Ok(json!({"role": "assistant", "content": content}))
}

/// The results of an assistant message's calls, in call order, as one user message.
fn results(calls: &[Call]) -> Value {
    let content: Vec<_> = calls
        .iter()
        .map(|call| tool_result(&call.id, call.message, call.result))
        .collect();
    json!({"role": "user", "content": content})
}

/// A result block holding the result text and then the images of the tool message that carries it.
fn tool_result(id: &str, message: &Message, result: &ToolResult) -> Value {
    let images = message.parts.iter().filter_map(|part| match part {
        Part::Image(image) => Some(image_block(image)),
        _ => None,
    });
    let content: Vec<_> = text_block(&result.content)
        .into_iter()
        .chain(images)
        .collect();
    json!({
        "type": "tool_result",
        "tool_use_id": id,
        "content": content,
        "is_error": result.is_error,
    })
}

/// A text block, or none for empty text, which the service rejects.
fn text_block(text: &str) -> Option<Value> {
    (!text.is_empty()).then(|| json!({"type": "text", "text": text}))
}

fn image_block(image: &Image) -> Value {
    json!({
        "type": "image",
        "source": {"type": "base64", "media_type": image.format.mime(), "data": image.base64()},
    })
}
