use enco_sdk::protocol::{
    Call, Request, Settings, Turn, apply_options, bad_request, call_id, turns,
    types::{Failure, Image, Message, Part, Role, ToolSpec},
};
use serde_json::{Value, json};

/// Gemini 3 rejects an unsigned call. Only calls from other services reach this point, since
/// this plugin's own replies replay whole, so they carry Google's documented placeholder.
const UNSIGNED: &str = "skip_thought_signature_validator";

/// Gemini rejects a function call turn that follows neither a user turn nor function responses.
/// A history that starts with one, as after a summary that ends mid-Run, gets this user turn
/// before it; it says only that earlier turns are left out.
const LEADING: &str = "(Earlier turns are not shown.)";

pub fn completion(settings: &Settings, request: Request) -> Result<Value, Failure> {
    let mut system = vec![];
    let mut contents = vec![];
    for turn in turns(&request.messages)? {
        match turn {
            Turn::Message(message) if message.role == Role::System => {
                system.extend(message.parts.iter().filter_map(|part| match part {
                    Part::Text(text) => Some(json!({"text": text})),
                    _ => None,
                }));
            }
            Turn::Message(message) => {
                contents.push(json!({"role": "user", "parts": user(message)?}));
            }
            Turn::Assistant {
                message,
                position,
                calls,
            } => {
                contents.push(model(message, position)?);
                if !calls.is_empty() {
                    contents.push(results(&calls));
                }
            }
        }
    }
    if contents.first().is_some_and(calls_first) {
        contents.insert(0, json!({"role": "user", "parts": [{"text": LEADING}]}));
    }
    let mut body = json!({
        "contents": contents,
        "safetySettings": safety(),
        "generationConfig": {"maxOutputTokens": request.max_output_tokens},
    });
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": system});
    }
    if !request.tools.is_empty() {
        let declarations = request
            .tools
            .into_iter()
            .map(declaration)
            .collect::<Result<Vec<_>, _>>()?;
        body["tools"] = json!([{"functionDeclarations": declarations}]);
    }
    apply_options(
        &mut body,
        &settings.options,
        &["contents", "systemInstruction", "tools"],
    )?;
    Ok(body)
}

/// Whether `content` is a model turn that calls a function.
fn calls_first(content: &Value) -> bool {
    content["role"] == "model"
        && content["parts"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part.get("functionCall").is_some()))
}

fn declaration(tool: ToolSpec) -> Result<Value, Failure> {
    let schema: Value =
        serde_json::from_str(&tool.input_schema).map_err(|error| bad_request(error.to_string()))?;
    Ok(json!({
        "name": tool.name,
        "description": tool.description,
        "parametersJsonSchema": schema,
    }))
}

/// The parts of a user turn: the message's text and images.
fn user(message: &Message) -> Result<Vec<Value>, Failure> {
    message
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => Ok(json!({"text": text})),
            Part::Image(image) => Ok(inline(image)),
            _ => Err(bad_request("a user message holds only text and images")),
        })
        .collect()
}

/// The model turn of the assistant message at `position`: the reply this plugin kept whole,
/// exactly as stored, or one built from the canonical parts.
fn model(message: &Message, position: usize) -> Result<Value, Failure> {
    if let Some(data) = message.parts.iter().find_map(|part| match part {
        Part::Extension(extension) => Some(&extension.data),
        _ => None,
    }) {
        return serde_json::from_str(data).map_err(|error| bad_request(error.to_string()));
    }
    let mut calls = 0;
    let parts = message
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => Ok(json!({"text": text})),
            Part::ToolCall(call) => {
                let args: Value = serde_json::from_str(&call.arguments)
                    .map_err(|error| bad_request(error.to_string()))?;
                let id = call_id(call, position, calls);
                calls += 1;
                let call = json!({"id": id, "name": call.name, "args": args});
                Ok(json!({"functionCall": call, "thoughtSignature": UNSIGNED}))
            }
            _ => Err(bad_request(
                "an assistant message holds only text and tool calls",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"role": "model", "parts": parts}))
}

/// The function responses to an assistant message's calls, as one user turn.
fn results(calls: &[Call]) -> Value {
    let parts: Vec<_> = calls.iter().map(function_response).collect();
    json!({"role": "user", "parts": parts})
}

fn function_response(call: &Call) -> Value {
    let result = call.result;
    let response = if result.is_error {
        json!({"error": result.content})
    } else {
        json!({"output": result.content})
    };
    let mut wire = json!({"name": call.call.name, "response": response});
    // Only a call in a reply sent whole can lack an ID, and the service pairs it by order.
    if !call.id.is_empty() {
        wire["id"] = json!(call.id);
    }
    let images = call
        .message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Image(image) => Some(inline(image)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !images.is_empty() {
        wire["parts"] = json!(images);
    }
    json!({"functionResponse": wire})
}

fn inline(image: &Image) -> Value {
    json!({"inlineData": {"mimeType": image.format.mime(), "data": image.base64()}})
}

fn safety() -> Vec<Value> {
    [
        "HARM_CATEGORY_HARASSMENT",
        "HARM_CATEGORY_HATE_SPEECH",
        "HARM_CATEGORY_SEXUALLY_EXPLICIT",
        "HARM_CATEGORY_DANGEROUS_CONTENT",
        "HARM_CATEGORY_CIVIC_INTEGRITY",
    ]
    .into_iter()
    .map(|category| json!({"category": category, "threshold": "OFF"}))
    .collect()
}

pub fn embedding(settings: &Settings, inputs: Vec<String>) -> Result<Value, Failure> {
    let model = format!("models/{}", settings.model);
    let requests = inputs
        .into_iter()
        .map(|text| {
            let content = json!({"parts": [{"text": text}]});
            let mut request = json!({"model": model, "content": content});
            apply_options(&mut request, &settings.options, &["content"])?;
            Ok(request)
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({"requests": requests}))
}
