//! The Responses request body for a canonical completion request: every message as input items in
//! its place, a kept reply's output items as they came, an ID on every call, a prompt cache key for
//! the conversation, and no service state.
use enco_sdk::protocol::{
    Call, Request, Settings, Turn, apply_options, bad_request, call_id,
    stream::streaming,
    turns,
    types::{Failure, Image, Message, Part, Role, ToolResult, ToolSpec},
};
use serde_json::{Value, json};

/// The fields this plugin writes or must leave out, which the owner's options cannot set.
///
/// `previous_response_id`, `conversation` and `background` keep state on the service, and
/// `truncation: auto` lets it drop context without saying so.
const RESERVED: &[&str] = &[
    "model",
    "input",
    "tools",
    "max_output_tokens",
    "store",
    "include",
    "previous_response_id",
    "conversation",
    "background",
    "truncation",
    "instructions",
    "prompt_cache_key",
];

/// The body of one stateless Responses request, and whether it asks for a stream, as it does
/// unless the options set `stream` to `false`.
///
/// Every request carries the whole history: the service stores nothing, and returns reasoning
/// encrypted so a later request can replay it.
pub fn completion(settings: &Settings, request: Request) -> Result<(Value, bool), Failure> {
    let (stream, options) = streaming(&settings.options)?;
    let mut input = vec![];
    for turn in turns(&request.messages)? {
        match turn {
            Turn::Message(message) if message.role == Role::System => {
                input.push(said("system", message)?);
            }
            Turn::Message(message) => input.push(said("user", message)?),
            Turn::Assistant {
                message,
                position,
                calls,
            } => {
                input.extend(assistant(message, position)?);
                input.extend(calls.iter().map(output));
            }
        }
    }
    let tools = request
        .tools
        .into_iter()
        .map(tool)
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = json!({
        "model": settings.model,
        "prompt_cache_key": cache_key(&settings.model, &tools, &input),
        "input": input,
        "max_output_tokens": request.max_output_tokens,
        "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if stream {
        body["stream"] = json!(true);
    }
    apply_options(&mut body, &options, RESERVED)?;
    Ok((body, stream))
}

/// The key that routes a request to the service's prompt cache.
///
/// It hashes what a conversation's requests keep in common, the model, the tools, the leading
/// system messages and the first user message, so each conversation keeps reaching the cache that
/// holds its beginning, and a compaction that rewrites that beginning starts a new key.
fn cache_key(model: &str, tools: &[Value], input: &[Value]) -> String {
    let system = input.iter().take_while(|item| item["role"] == "system");
    let head: Vec<_> = system
        .chain(input.iter().find(|item| item["role"] == "user"))
        .collect();
    let seed = json!([model, tools, head]).to_string();
    format!("enco-{}", &blake3::hash(seed.as_bytes()).to_hex()[..32])
}

/// A function tool, with `strict` written as `false`: left out, the service rewrites the schema
/// into its strict form, and the model reads constraints other than the declared ones.
fn tool(tool: ToolSpec) -> Result<Value, Failure> {
    let parameters: Value =
        serde_json::from_str(&tool.input_schema).map_err(|error| bad_request(error.to_string()))?;
    Ok(json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": parameters,
        "strict": false,
    }))
}

/// A system or user message as a `message` item of input text and images.
fn said(role: &str, message: &Message) -> Result<Value, Failure> {
    let content = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => input_text(text).map(Ok),
            Part::Image(image) => Some(Ok(input_image(image))),
            _ => Some(Err(bad_request(format!(
                "a {role} message holds only text and images"
            )))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"type": "message", "role": role, "content": content}))
}

/// The input items of the assistant message at `position`: the output items this plugin kept
/// whole, exactly as stored, or one item for each canonical part in order.
fn assistant(message: &Message, position: usize) -> Result<Vec<Value>, Failure> {
    if let Some(data) = message.parts.iter().find_map(|part| match part {
        Part::Extension(extension) => Some(&extension.data),
        _ => None,
    }) {
        return serde_json::from_str(data).map_err(|error| bad_request(error.to_string()));
    }
    let mut items = vec![];
    let mut calls = 0;
    for part in &message.parts {
        match part {
            Part::Text(text) if text.is_empty() => {}
            Part::Text(text) => items.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            })),
            Part::ToolCall(call) => {
                items.push(json!({
                    "type": "function_call",
                    "call_id": call_id(call, position, calls),
                    "name": call.name,
                    "arguments": call.arguments,
                }));
                calls += 1;
            }
            _ => {
                return Err(bad_request(
                    "an assistant message holds only text and tool calls",
                ));
            }
        }
    }
    Ok(items)
}

/// The `function_call_output` item that answers `call`.
fn output(call: &Call) -> Value {
    json!({
        "type": "function_call_output",
        "call_id": call.id,
        "output": result(call.message, call.result),
    })
}

/// The result text, or, when the tool message carries images, the text followed by those images.
///
/// The item has no error flag, so a failed call is told only by its result text.
fn result(message: &Message, result: &ToolResult) -> Value {
    let images: Vec<_> = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Image(image) => Some(input_image(image)),
            _ => None,
        })
        .collect();
    if images.is_empty() {
        return json!(result.content);
    }
    let content: Vec<_> = input_text(&result.content)
        .into_iter()
        .chain(images)
        .collect();
    json!(content)
}

/// An input text entry, or none for empty text, which says nothing.
fn input_text(text: &str) -> Option<Value> {
    (!text.is_empty()).then(|| json!({"type": "input_text", "text": text}))
}

fn input_image(image: &Image) -> Value {
    let url = format!("data:{};base64,{}", image.format.mime(), image.base64());
    json!({"type": "input_image", "image_url": url})
}
