use crate::bindings::{enco::plugin::types as wit, exports::enco::plugin::completion as wire};
use enco_core::*;
use enco_kernel::{Completion, ProviderRequest};

pub(crate) fn request(request: ProviderRequest) -> wire::Request {
    wire::Request {
        messages: request.messages.into_iter().map(message).collect(),
        tools: request
            .tools
            .into_iter()
            .map(|tool| wire::ToolSpec {
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema.to_string(),
            })
            .collect(),
        max_output_tokens: request.max_output_tokens,
    }
}

fn message(message: Message) -> wit::Message {
    let role = match message.role {
        Role::System => wit::Role::System,
        Role::User => wit::Role::User,
        Role::Assistant => wit::Role::Assistant,
        Role::Tool => wit::Role::Tool,
    };
    let parts = message
        .parts
        .into_iter()
        .map(|part| match part {
            Part::Text { text } => wit::Part::Text(text),
            Part::ToolCall(call) => wit::Part::ToolCall(wit::ToolCall {
                id: call.provider_id,
                name: call.name,
                arguments: call.arguments,
            }),
            Part::ToolResult(result) => wit::Part::ToolResult(wit::ToolResult {
                call_id: result.provider_id,
                content: result.content,
                is_error: result.is_error,
            }),
            Part::Extension(extension) => wit::Part::Extension(wit::Extension {
                data: extension.data.to_string(),
            }),
        })
        .collect();
    wit::Message { role, parts }
}

pub(crate) fn failure(failure: wit::Failure) -> Failure {
    Failure {
        code: failure.code,
        message: failure.message,
        retryable: failure.retryable,
    }
}

pub(crate) fn completion(completion: wire::Completion) -> Result<Completion, Failure> {
    if completion.message.role != wit::Role::Assistant {
        return Err(bad("completion role must be assistant"));
    }
    let parts = completion
        .message
        .parts
        .into_iter()
        .map(|part| {
            Ok(match part {
                wit::Part::Text(text) => Part::Text { text },
                wit::Part::ToolCall(call) => Part::ToolCall(ToolCall {
                    id: CallId::new(),
                    provider_id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                }),
                wit::Part::Extension(extension) => Part::Extension(Extension {
                    data: serde_json::from_str(&extension.data).map_err(|e| bad(e.to_string()))?,
                }),
                wit::Part::ToolResult(_) => {
                    return Err(bad("assistant completion cannot contain a tool result"));
                }
            })
        })
        .collect::<Result<_, Failure>>()?;
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts,
        },
        usage: Usage {
            input_tokens: completion.usage.input_tokens,
            output_tokens: completion.usage.output_tokens,
            cached_input_tokens: completion.usage.cached_input_tokens,
        },
        stop: match completion.stop_reason {
            wire::StopReason::EndTurn => StopReason::EndTurn,
            wire::StopReason::ToolCalls => StopReason::ToolCalls,
            wire::StopReason::MaxTokens => StopReason::MaxTokens,
            wire::StopReason::Other => StopReason::Other,
        },
    })
}

pub(crate) fn bad(message: impl Into<String>) -> Failure {
    Failure {
        code: code::PLUGIN_CONTRACT.into(),
        message: message.into(),
        retryable: false,
    }
}

pub(crate) fn embeddings(vectors: Vec<Vec<f32>>, count: usize) -> Result<Vec<Vec<f32>>, Failure> {
    let width = vectors.first().map(Vec::len);
    if vectors.len() != count
        || vectors.iter().any(|vector| {
            vector.is_empty()
                || Some(vector.len()) != width
                || vector.iter().any(|value| !value.is_finite())
        })
    {
        return Err(bad(
            "embedding count, widths or numeric values violate the interface contract",
        ));
    }
    Ok(vectors)
}
