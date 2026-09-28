//! Shared Chat Completions wire protocol. No host or kernel crate is visible here.
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "provider-plugin",
        pub_export_macro: true,
    });
}

pub use bindings::enco::plugin::{host, types};
pub use bindings::exports::enco::plugin::provider::{
    Completion, Request, Settings, StopReason, Usage,
};
use serde_json::{Map, Value, json};
use types::{Failure, Message, Part, Role};

pub fn failure(code: &str, message: impl Into<String>, retryable: bool) -> Failure {
    Failure {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn bad(message: impl Into<String>) -> Failure {
    failure("provider.bad_response", message, false)
}

fn apply_options(body: &mut Value, raw: &str, reserved: &[&str]) -> Result<(), Failure> {
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        failure(
            "provider.bad_request",
            format!("invalid options: {e}"),
            false,
        )
    })?;
    let map = value
        .as_object()
        .ok_or_else(|| failure("provider.bad_request", "options must be an object", false))?;
    for (name, value) in map {
        if reserved.contains(&name.as_str()) {
            return Err(failure(
                "provider.bad_request",
                format!("options cannot override {name}"),
                false,
            ));
        }
        body[name] = value.clone();
    }
    Ok(())
}

pub async fn complete(
    settings: Settings,
    request: Request,
    provider: &str,
) -> Result<Completion, Failure> {
    let messages = request
        .messages
        .iter()
        .map(|m| message(m, provider))
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = json!({ "model": settings.model, "messages": messages });
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                let parameters = serde_json::from_str::<Value>(&tool.input_schema)
                    .map_err(|e| failure("provider.bad_request", e.to_string(), false))?;
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": parameters,
                    },
                }))
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        body["tools"] = json!(tools);
    }
    if let Some(max) = request.max_output_tokens {
        body["max_tokens"] = json!(max);
    }
    apply_options(
        &mut body,
        &settings.options,
        &[
            "model",
            "messages",
            "tools",
            "max_tokens",
            "max_completion_tokens",
            "stream",
            "stream_options",
        ],
    )?;
    parse_completion(
        post(&settings, "chat/completions", body, None).await?,
        provider,
    )
}

pub async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
    let count = inputs.len();
    let mut body = json!({ "model": settings.model, "input": inputs });
    apply_options(
        &mut body,
        &settings.options,
        &["model", "input", "encoding_format"],
    )?;
    let response = post(&settings, "embeddings", body, Some(30000)).await?;
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("missing embeddings data"))?;
    if data.len() != count {
        return Err(bad("embedding count does not match input count"));
    }
    let mut embeddings: Vec<_> = data
        .iter()
        .map(|embedding| {
            // Compatible APIs may omit a scalar's default zero value.
            // Coverage validation below still rejects duplicate or missing positions.
            let index = match embedding.get("index") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| bad("invalid embedding index"))?,
            };
            let vector: Vec<f32> = serde_json::from_value(
                embedding
                    .get("embedding")
                    .cloned()
                    .ok_or_else(|| bad("missing embedding"))?,
            )
            .map_err(|e| bad(e.to_string()))?;
            if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
                return Err(bad("embedding must contain finite numbers"));
            }
            Ok((index, vector))
        })
        .collect::<Result<_, Failure>>()?;
    embeddings.sort_by_key(|(index, _)| *index);
    if embeddings
        .iter()
        .enumerate()
        .any(|(i, (index, _))| *index != i as u64)
    {
        return Err(bad("embedding indexes must cover the input once"));
    }
    Ok(embeddings.into_iter().map(|(_, vector)| vector).collect())
}

fn message(message: &Message, provider: &str) -> Result<Value, Failure> {
    let text = message
        .parts
        .iter()
        .filter_map(|p| {
            if let Part::Text(t) = p {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(match message.role {
        Role::System => json!({ "role": "system", "content": text }),
        Role::User => json!({ "role": "user", "content": text }),
        Role::Tool => {
            let [Part::ToolResult(result)] = message.parts.as_slice() else {
                return Err(failure(
                    "provider.bad_request",
                    "tool message must have one result",
                    false,
                ));
            };
            json!({ "role": "tool", "tool_call_id": result.call_id, "content": result.content })
        }
        Role::Assistant => {
            let content = if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            };
            let mut wire = json!({ "role": "assistant", "content": content });
            let calls: Vec<_> = message
                .parts
                .iter()
                .filter_map(|part| {
                    let Part::ToolCall(call) = part else {
                        return None;
                    };
                    Some(json!({
                        "id": call.id,
                        "type": "function",
                        "function": { "name": call.name, "arguments": call.arguments },
                    }))
                })
                .collect();
            if !calls.is_empty() {
                wire["tool_calls"] = json!(calls);
            }
            for part in &message.parts {
                if let Part::Extension(extension) = part
                    && extension.provider == provider
                {
                    let fields: Map<String, Value> = serde_json::from_str(&extension.data)
                        .map_err(|e| failure("provider.bad_request", e.to_string(), false))?;
                    for (key, value) in fields {
                        if ["role", "content", "tool_calls"].contains(&key.as_str()) {
                            return Err(failure(
                                "provider.bad_request",
                                "extension conflicts with canonical message fields",
                                false,
                            ));
                        }
                        wire[key] = value;
                    }
                }
            }
            wire
        }
    })
}

fn parse_completion(response: Value, provider: &str) -> Result<Completion, Failure> {
    let choice = response
        .pointer("/choices/0")
        .ok_or_else(|| bad("missing choices[0]"))?;
    let message = parse_message(choice.get("message"), provider)?;
    let usage = parse_usage(response.get("usage"))?;
    let stop_reason = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") => StopReason::ToolCalls,
        Some("length") => StopReason::MaxTokens,
        _ => StopReason::Other,
    };
    Ok(Completion {
        message,
        usage,
        stop_reason,
    })
}

fn parse_message(value: Option<&Value>, provider: &str) -> Result<Message, Failure> {
    let mut wire = value
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| bad("missing assistant message"))?;
    match wire.remove("role") {
        Some(Value::String(role)) if role == "assistant" => {}
        _ => return Err(bad("completion role must be assistant")),
    }
    let mut parts = vec![];
    match wire.remove("content") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => parts.push(Part::Text(text)),
        Some(_) => return Err(bad("completion content must be text or null")),
    }
    let calls = match wire.remove("tool_calls") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(calls)) => calls,
        Some(_) => return Err(bad("tool_calls must be an array")),
    };
    let mut ids = std::collections::HashSet::new();
    for call in calls {
        let id = field(&call, "/id")?;
        if !ids.insert(id.clone()) {
            return Err(bad("duplicate provider call ID"));
        }
        parts.push(Part::ToolCall(types::ToolCall {
            id,
            name: field(&call, "/function/name")?,
            arguments: field(&call, "/function/arguments")?,
        }));
    }
    if !wire.is_empty() {
        parts.push(Part::Extension(types::Extension {
            provider: provider.into(),
            data: Value::Object(wire).to_string(),
        }));
    }
    Ok(Message {
        role: Role::Assistant,
        parts,
    })
}

fn parse_usage(value: Option<&Value>) -> Result<Usage, Failure> {
    let usage = value.ok_or_else(|| bad("missing usage"))?;
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("missing prompt_tokens"))?;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("missing completion_tokens"))?;
    let cached_input_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64);
    Ok(Usage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
    })
}

fn field(value: &Value, path: &str) -> Result<String, Failure> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| bad(format!("missing text at {path}")))
}

async fn post(
    settings: &Settings,
    path: &str,
    body: Value,
    timeout_ms: Option<u32>,
) -> Result<Value, Failure> {
    let mut headers = vec![("Content-Type".into(), "application/json".into())];
    if let Some(key) = &settings.api_key {
        headers.push(("Authorization".into(), format!("Bearer {key}")));
    }
    let response = host::http(host::HttpRequest {
        method: "POST".into(),
        url: format!("{}/{}", settings.base_url.trim_end_matches('/'), path),
        headers,
        body: Some(body.to_string().into_bytes()),
        timeout_ms,
    })
    .await
    .map_err(|e| {
        failure(
            if e.kind == host::HttpErrorKind::Timeout {
                "timeout"
            } else {
                "provider.network"
            },
            e.message,
            true,
        )
    })?;
    if !(200..300).contains(&response.status) {
        let text = String::from_utf8_lossy(&response.body);
        let mut excerpt = text.into_owned();
        if let Some(key) = &settings.api_key
            && !key.is_empty()
        {
            excerpt = excerpt.replace(key, "[redacted]");
        }
        excerpt.truncate(excerpt.floor_char_boundary(500));
        let (code, retryable) = match response.status {
            401 | 403 => ("provider.auth", false),
            429 => ("provider.rate_limited", true),
            500..=599 => ("provider.server", true),
            _ => ("provider.bad_request", false),
        };
        return Err(failure(
            code,
            format!("HTTP {}: {excerpt}", response.status),
            retryable,
        ));
    }
    serde_json::from_slice(&response.body).map_err(|e| bad(format!("invalid JSON response: {e}")))
}
