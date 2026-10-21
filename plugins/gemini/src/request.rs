use enco_sdk::protocol::{
    Request, Settings, apply_options, bad_request,
    types::{Failure, Image, Message, Part, Role, ToolCall, ToolResult, ToolSpec},
};
use serde_json::{Value, json};

/// Gemini 3 rejects an unsigned call. Only calls from other services reach this point, since
/// this plugin's own replies replay whole, so they carry Google's documented placeholder.
const UNSIGNED: &str = "skip_thought_signature_validator";

pub fn completion(settings: &Settings, request: Request) -> Result<Value, Failure> {
    let mut system = vec![];
    let mut contents = vec![];
    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            Role::System => system.extend(message.parts.iter().filter_map(|part| match part {
                Part::Text(text) => Some(json!({"text": text})),
                _ => None,
            })),
            Role::User => contents.push(content(message)?),
            Role::Assistant => {
                contents.push(content(message)?);
                let tools: Vec<_> = request.messages[index + 1..]
                    .iter()
                    .take_while(|next| next.role == Role::Tool)
                    .collect();
                if !tools.is_empty() {
                    contents.push(results(message, &tools)?);
                }
            }
            // Taken with the assistant message whose calls they answer.
            Role::Tool => {}
        }
    }
    let mut body = json!({"contents": contents, "safetySettings": safety()});
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
    if let Some(max) = request.max_output_tokens {
        body["generationConfig"] = json!({"maxOutputTokens": max});
    }
    apply_options(
        &mut body,
        &settings.options,
        &["contents", "systemInstruction", "tools"],
    )?;
    Ok(body)
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

fn content(message: &Message) -> Result<Value, Failure> {
    if message.role == Role::Assistant {
        for part in &message.parts {
            if let Part::Extension(extension) = part {
                return serde_json::from_str(&extension.data)
                    .map_err(|error| bad_request(error.to_string()));
            }
        }
    }
    let parts = message
        .parts
        .iter()
        .map(wire_part)
        .collect::<Result<Vec<_>, _>>()?;
    let role = if message.role == Role::Assistant {
        "model"
    } else {
        "user"
    };
    Ok(json!({"role": role, "parts": parts}))
}

fn wire_part(part: &Part) -> Result<Value, Failure> {
    match part {
        Part::Text(text) => Ok(json!({"text": text})),
        Part::Image(image) => Ok(inline(image)),
        Part::ToolCall(call) => {
            let args: Value = serde_json::from_str(&call.arguments)
                .map_err(|error| bad_request(error.to_string()))?;
            let call = json!({"id": call.id, "name": call.name, "args": args});
            Ok(json!({"functionCall": call, "thoughtSignature": UNSIGNED}))
        }
        _ => Err(bad_request("unexpected part outside a tool result batch")),
    }
}

/// The protocol pairs a call and its result by ID, or by order when the service gave the call
/// none; the kernel settles results in call order.
fn results(assistant: &Message, tools: &[&Message]) -> Result<Value, Failure> {
    let results: Vec<_> = tools
        .iter()
        .filter_map(|message| match message.parts.first() {
            Some(Part::ToolResult(result)) => Some((*message, result)),
            _ => None,
        })
        .collect();
    let mut unnamed = results
        .iter()
        .filter(|(_, result)| result.call_id.is_empty());
    let parts = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
        .map(|call| {
            let found = if call.id.is_empty() {
                unnamed.next()
            } else {
                results.iter().find(|(_, result)| result.call_id == call.id)
            };
            let (message, result) =
                found.ok_or_else(|| bad_request(format!("missing result for {}", call.name)))?;
            Ok(function_response(call, message, result))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({"role": "user", "parts": parts}))
}

fn function_response(call: &ToolCall, message: &Message, result: &ToolResult) -> Value {
    let response = if result.is_error {
        json!({"error": result.content})
    } else {
        json!({"output": result.content})
    };
    let mut wire = json!({"name": call.name, "response": response});
    if !call.id.is_empty() {
        wire["id"] = json!(call.id);
    }
    let images = message
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
