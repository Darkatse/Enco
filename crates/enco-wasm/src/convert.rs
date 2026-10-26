//! Translates between the kernel's types and the WIT types.
//!
//! Values from a guest are checked on the way in; one that breaks the contract, such as an
//! embedding batch that does not match its inputs, fails as `plugin.contract`.
use crate::{
    bindings::{decisions, enco::plugin::types as wit, exports::enco::plugin::completion as wire},
    limits::DECISION_SUM_TOLERANCE,
};
use enco_core::*;
use enco_kernel::{Answer, Completion, Label, ProviderRequest, Question, QuestionKind};

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
                effect: tool.effect.into(),
            })
            .collect(),
        max_output_tokens: request.max_output_tokens,
    }
}

fn message(message: Message<ImageData>) -> wit::Message {
    let role = message.role.into();
    let parts = message
        .parts
        .into_iter()
        .map(|part| match part {
            Part::Text { text } => wit::Part::Text(text),
            Part::Image(image) => wit::Part::Image(wit::Image {
                format: image.format.into(),
                data: image.data,
            }),
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
                wit::Part::Image(_) => {
                    return Err(bad("assistant completion cannot contain a canonical image"));
                }
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

pub(crate) fn question(question: &Question) -> decisions::Question {
    decisions::Question {
        instructions: question.instructions.clone(),
        kind: match &question.kind {
            QuestionKind::Predicate => decisions::QuestionKind::Predicate,
            QuestionKind::Choice(labels) => {
                decisions::QuestionKind::Choice(decision_labels(labels))
            }
            QuestionKind::Score(labels) => decisions::QuestionKind::Score(decision_labels(labels)),
        },
    }
}

fn decision_labels(labels: &[Label]) -> Vec<decisions::Label> {
    labels
        .iter()
        .map(|label| decisions::Label {
            name: label.name.clone(),
            description: label.description.clone(),
        })
        .collect()
}

pub(crate) fn answers(
    answers: Vec<decisions::Answer>,
    questions: &[Question],
) -> Result<Vec<Answer>, Failure> {
    if answers.len() != questions.len() {
        return Err(bad("decision answer count does not match question count"));
    }
    let mut converted = Vec::with_capacity(answers.len());
    for (index, (answer, question)) in answers.into_iter().zip(questions).enumerate() {
        let answer = match (answer, &question.kind) {
            (decisions::Answer::Refused, _) => Answer::Refused,
            (decisions::Answer::Predicate(p), QuestionKind::Predicate)
                if (0.0..=1.0).contains(&p) =>
            {
                Answer::Predicate(p)
            }
            (decisions::Answer::Choice(p), QuestionKind::Choice(labels))
                if distribution(&p, labels.len()) =>
            {
                Answer::Choice(p)
            }
            (decisions::Answer::Score(p), QuestionKind::Score(labels))
                if distribution(&p, labels.len()) =>
            {
                Answer::Score(p)
            }
            _ => {
                return Err(bad(format!(
                    "decision answer {index}: invalid type or probabilities"
                )));
            }
        };
        converted.push(answer);
    }
    Ok(converted)
}

fn distribution(probabilities: &[f64], count: usize) -> bool {
    probabilities.len() == count
        && probabilities.iter().all(|p| (0.0..=1.0).contains(p))
        && (probabilities.iter().sum::<f64>() - 1.0).abs() <= DECISION_SUM_TOLERANCE
}

impl From<Role> for wit::Role {
    fn from(role: Role) -> Self {
        match role {
            Role::System => Self::System,
            Role::User => Self::User,
            Role::Assistant => Self::Assistant,
            Role::Tool => Self::Tool,
        }
    }
}

impl From<wit::Role> for Role {
    fn from(role: wit::Role) -> Self {
        match role {
            wit::Role::System => Self::System,
            wit::Role::User => Self::User,
            wit::Role::Assistant => Self::Assistant,
            wit::Role::Tool => Self::Tool,
        }
    }
}

impl From<ImageFormat> for wit::ImageFormat {
    fn from(format: ImageFormat) -> Self {
        match format {
            ImageFormat::Png => Self::Png,
            ImageFormat::Jpeg => Self::Jpeg,
            ImageFormat::Webp => Self::Webp,
        }
    }
}

impl From<Effect> for wit::Effect {
    fn from(effect: Effect) -> Self {
        match effect {
            Effect::ReadOnly => Self::ReadOnly,
            Effect::Idempotent => Self::Idempotent,
            Effect::SideEffect => Self::SideEffect,
        }
    }
}

impl From<wit::Effect> for Effect {
    fn from(effect: wit::Effect) -> Self {
        match effect {
            wit::Effect::ReadOnly => Self::ReadOnly,
            wit::Effect::Idempotent => Self::Idempotent,
            wit::Effect::SideEffect => Self::SideEffect,
        }
    }
}
