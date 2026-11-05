use super::wire_call;
use crate::protocol::{Turn, bad_request, call_id, turns};
use crate::types::{Failure, Image, Message, Part, Role};
use serde_json::{Value, json};

/// The Chat Completions messages of a request. A tool message carries only text, so the images of
/// a batch of results follow it in one user message.
pub(super) fn encode(messages: &[Message]) -> Result<Vec<Value>, Failure> {
    let mut encoded = vec![];
    for turn in turns(messages)? {
        match turn {
            Turn::Message(message) if message.role == Role::System => {
                encoded.push(json!({ "role": "system", "content": joined_text(&message.parts) }))
            }
            Turn::Message(message) => {
                encoded.push(json!({ "role": "user", "content": content(&message.parts) }));
            }
            Turn::Assistant {
                message,
                position,
                calls,
            } => {
                encoded.push(assistant(message, position)?);
                let mut images = vec![];
                for call in &calls {
                    encoded.push(json!({
                        "role": "tool",
                        "tool_call_id": call.id,
                        "content": call.result.content,
                    }));
                    images.extend(call.message.parts.iter().filter_map(|part| match part {
                        Part::Image(image) => Some(wire_image(image)),
                        _ => None,
                    }));
                }
                if !images.is_empty() {
                    encoded.push(json!({"role": "user", "content": images}));
                }
            }
        }
    }
    Ok(encoded)
}

fn joined_text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn content(parts: &[Part]) -> Value {
    if !parts.iter().any(|part| matches!(part, Part::Image(_))) {
        return json!(joined_text(parts));
    }
    json!(
        parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(json!({"type": "text", "text": text})),
                Part::Image(image) => Some(wire_image(image)),
                _ => None,
            })
            .collect::<Vec<_>>()
    )
}

fn wire_image(image: &Image) -> Value {
    let url = format!("data:{};base64,{}", image.format.mime(), image.base64());
    json!({"type": "image_url", "image_url": {"url": url}})
}

/// The assistant message at `position`: a reply kept whole exactly as the service wrote it, or one
/// built from the canonical parts.
pub(super) fn assistant(message: &Message, position: usize) -> Result<Value, Failure> {
    if let Some(Part::Extension(extension)) = message
        .parts
        .iter()
        .find(|part| matches!(part, Part::Extension(_)))
    {
        return serde_json::from_str(&extension.data).map_err(|e| bad_request(e.to_string()));
    }
    let text = joined_text(&message.parts);
    let calls: Vec<_> = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
        .enumerate()
        .map(|(index, call)| wire_call(&call_id(call, position, index), call))
        .collect();
    // Chat Completions allows a null content only beside tool calls.
    let content = if text.is_empty() && !calls.is_empty() {
        Value::Null
    } else {
        json!(text)
    };
    let mut wire = json!({ "role": "assistant", "content": content });
    if !calls.is_empty() {
        wire["tool_calls"] = json!(calls);
    }
    Ok(wire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageFormat, ToolCall, ToolResult};

    #[test]
    fn tool_images_follow_the_complete_result_batch() -> Result<(), Failure> {
        let image = || {
            Part::Image(Image {
                format: ImageFormat::Png,
                data: vec![1, 2, 3],
            })
        };
        let tool = |id: &str| Message {
            role: Role::Tool,
            parts: vec![
                Part::ToolResult(ToolResult {
                    call_id: id.into(),
                    content: id.into(),
                    is_error: false,
                }),
                image(),
            ],
        };
        let call = |id: &str| {
            Part::ToolCall(ToolCall {
                id: id.into(),
                name: "read".into(),
                arguments: "{}".into(),
            })
        };
        let messages = encode(&[
            Message {
                role: Role::Assistant,
                parts: vec![call("first"), call("second")],
            },
            tool("first"),
            tool("second"),
            Message {
                role: Role::User,
                parts: vec![Part::Text("next".into())],
            },
        ])?;
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[1]["tool_call_id"], "first");
        assert_eq!(messages[2]["tool_call_id"], "second");
        assert_eq!(messages[3]["content"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            messages[3]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
        assert_eq!(messages[4]["content"], "next");
        let user = encode(&[Message {
            role: Role::User,
            parts: vec![image(), Part::Text("describe".into())],
        }])?;
        assert_eq!(user[0]["content"][0]["type"], "image_url");
        assert_eq!(user[0]["content"][1]["text"], "describe");
        Ok(())
    }

    #[test]
    fn a_reply_with_service_fields_goes_back_as_it_came() -> Result<(), Failure> {
        let call = |id: &str| {
            let function = json!({"name": "read", "arguments": "{}"});
            json!({"id": id, "type": "function", "function": function})
        };
        // Parallel Gemini calls sign only the first one.
        let mut signed = call("a");
        signed["extra_content"] = json!({"google": {"thought_signature": "signed"}});
        let reply =
            json!({"role": "assistant", "content": null, "tool_calls": [signed, call("b")]});
        let parsed = super::super::parse_message(Some(&reply))?;
        assert_eq!(assistant(&parsed, 0)?, reply);
        Ok(())
    }

    #[test]
    fn empty_service_fields_keep_a_reply_canonical() -> Result<(), Failure> {
        let reply = json!({
            "role": "assistant",
            "content": "hello",
            "refusal": null,
            "annotations": [],
        });
        let parsed = super::super::parse_message(Some(&reply))?;
        assert!(
            !parsed
                .parts
                .iter()
                .any(|part| matches!(part, Part::Extension(_)))
        );
        assert_eq!(
            assistant(&parsed, 0)?,
            json!({"role": "assistant", "content": "hello"})
        );
        Ok(())
    }
}
