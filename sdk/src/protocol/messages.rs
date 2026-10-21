use super::{bad_request, wire_call};
use crate::http::merge;
use crate::types::{Failure, Image, Message, Part, Role};
use serde_json::{Map, Value, json};

pub(super) fn encode(messages: &[Message]) -> Result<Vec<Value>, Failure> {
    let mut encoded = vec![];
    let mut images = vec![];
    for item in messages {
        if item.role != Role::Tool && !images.is_empty() {
            encoded.push(json!({"role": "user", "content": std::mem::take(&mut images)}));
        }
        encoded.push(message(item)?);
        if item.role == Role::Tool {
            images.extend(item.parts.iter().filter_map(|part| match part {
                Part::Image(image) => Some(wire_image(image)),
                _ => None,
            }));
        }
    }
    if !images.is_empty() {
        encoded.push(json!({"role": "user", "content": images}));
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

fn message(message: &Message) -> Result<Value, Failure> {
    Ok(match message.role {
        Role::System => json!({ "role": "system", "content": joined_text(&message.parts) }),
        Role::User => json!({ "role": "user", "content": content(&message.parts) }),
        Role::Tool => {
            let Some(Part::ToolResult(result)) = message.parts.first() else {
                return Err(bad_request("tool message must start with its result"));
            };
            json!({ "role": "tool", "tool_call_id": result.call_id, "content": result.content })
        }
        Role::Assistant => {
            let text = joined_text(&message.parts);
            let calls: Vec<_> = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::ToolCall(call) => Some(wire_call(call)),
                    _ => None,
                })
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
            for part in &message.parts {
                if let Part::Extension(extension) = part {
                    let rest: Map<String, Value> = serde_json::from_str(&extension.data)
                        .map_err(|e| bad_request(e.to_string()))?;
                    merge(&mut wire, Value::Object(rest)).map_err(|path| {
                        bad_request(format!("extension conflicts with canonical field {path}"))
                    })?;
                }
            }
            wire
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageFormat, ToolResult};

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
        let messages = encode(&[
            tool("first"),
            tool("second"),
            Message {
                role: Role::User,
                parts: vec![Part::Text("next".into())],
            },
        ])?;
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["tool_call_id"], "first");
        assert_eq!(messages[1]["tool_call_id"], "second");
        assert_eq!(messages[2]["content"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            messages[2]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
        assert_eq!(messages[3]["content"], "next");
        let user = encode(&[Message {
            role: Role::User,
            parts: vec![image(), Part::Text("describe".into())],
        }])?;
        assert_eq!(user[0]["content"][0]["type"], "image_url");
        assert_eq!(user[0]["content"][1]["text"], "describe");
        Ok(())
    }

    #[test]
    fn a_call_residue_returns_to_its_own_call() -> Result<(), Failure> {
        let call = |id: &str| {
            let function = json!({"name": "read", "arguments": "{}"});
            json!({"id": id, "type": "function", "function": function})
        };
        // Parallel Gemini calls sign only the first one.
        let mut signed = call("a");
        signed["extra_content"] = json!({"google": {"thought_signature": "signed"}});
        let reply =
            json!({"role": "assistant", "content": null, "tool_calls": [signed, call("b")]});
        let parsed = crate::protocol::parse_message(Some(&reply))?;
        assert_eq!(message(&parsed)?["tool_calls"], json!([signed, call("b")]));
        Ok(())
    }
}
