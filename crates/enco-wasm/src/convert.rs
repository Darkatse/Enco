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
    let ProviderRequest {
        messages,
        tools,
        max_output_tokens,
    } = request;
    wire::Request {
        messages: messages.into_iter().map(message).collect(),
        tools: tools
            .into_iter()
            .map(
                |DisclosedTool {
                     name,
                     description,
                     input_schema,
                     effect,
                 }| wire::ToolSpec {
                    name,
                    description,
                    input_schema: input_schema.to_string(),
                    effect: effect.into(),
                },
            )
            .collect(),
        max_output_tokens,
    }
}

fn message(message: Message<ImageData>) -> wit::Message {
    let Message { role, parts } = message;
    let role = role.into();
    let parts = parts
        .into_iter()
        .map(|part| match part {
            Part::Text { text } => wit::Part::Text(text),
            Part::Image(ImageData { format, data }) => wit::Part::Image(wit::Image {
                format: format.into(),
                data,
            }),
            Part::ToolCall(ToolCall {
                id: _,
                provider_id,
                name,
                arguments,
            }) => wit::Part::ToolCall(wit::ToolCall {
                id: provider_id,
                name,
                arguments,
            }),
            Part::ToolResult(ToolResult {
                call: _,
                provider_id,
                content,
                is_error,
            }) => wit::Part::ToolResult(wit::ToolResult {
                call_id: provider_id,
                content,
                is_error,
            }),
            Part::Extension(Extension { data }) => wit::Part::Extension(wit::Extension {
                data: data.to_string(),
            }),
        })
        .collect();
    wit::Message { role, parts }
}

pub(crate) fn failure(failure: wit::Failure) -> Failure {
    let wit::Failure {
        code,
        message,
        retryable,
    } = failure;
    Failure {
        code,
        message,
        retryable,
    }
}

pub(crate) fn failure_record(failure: &Failure) -> wit::Failure {
    let Failure {
        code,
        message,
        retryable,
    } = failure;
    wit::Failure {
        code: code.clone(),
        message: message.clone(),
        retryable: *retryable,
    }
}

pub(crate) fn run_end(end: &RunEnd) -> wit::RunEnd {
    match end {
        RunEnd::Completed => wit::RunEnd::Completed,
        RunEnd::BudgetExhausted => wit::RunEnd::BudgetExhausted,
        RunEnd::Interrupted => wit::RunEnd::Interrupted,
        RunEnd::Cancelled => wit::RunEnd::Cancelled,
        RunEnd::Backtracked => wit::RunEnd::Backtracked,
        RunEnd::Halted => wit::RunEnd::Halted,
        RunEnd::Failed { failure } => wit::RunEnd::Failed(failure_record(failure)),
    }
}

pub(crate) fn usage_record(usage: &Usage) -> wit::Usage {
    let Usage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
    } = usage;
    wit::Usage {
        input_tokens: *input_tokens,
        output_tokens: *output_tokens,
        cached_input_tokens: *cached_input_tokens,
    }
}

pub(crate) fn completion(completion: wire::Completion) -> Result<Completion, Failure> {
    let wire::Completion {
        message,
        usage,
        stop_reason,
    } = completion;
    let wit::Message { role, parts } = message;
    let wit::Usage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
    } = usage;
    if role != wit::Role::Assistant {
        return Err(bad("completion role must be assistant"));
    }
    let parts = parts
        .into_iter()
        .map(|part| {
            Ok(match part {
                wit::Part::Text(text) => Part::Text { text },
                wit::Part::ToolCall(wit::ToolCall {
                    id,
                    name,
                    arguments,
                }) => Part::ToolCall(ToolCall {
                    id: CallId::new(),
                    provider_id: id,
                    name,
                    arguments,
                }),
                wit::Part::Extension(wit::Extension { data }) => Part::Extension(Extension {
                    data: serde_json::from_str(&data).map_err(|e| bad(e.to_string()))?,
                }),
                wit::Part::Image(wit::Image { format: _, data: _ }) => {
                    return Err(bad("assistant completion cannot contain a canonical image"));
                }
                wit::Part::ToolResult(wit::ToolResult {
                    call_id: _,
                    content: _,
                    is_error: _,
                }) => {
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
            input_tokens,
            output_tokens,
            cached_input_tokens,
        },
        stop: match stop_reason {
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
    let Question { instructions, kind } = question;
    decisions::Question {
        instructions: instructions.clone(),
        kind: match kind {
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
        .map(|Label { name, description }| decisions::Label {
            name: name.clone(),
            description: description.clone(),
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
        let Question {
            instructions: _,
            kind,
        } = question;
        let answer = match (answer, kind) {
            (
                decisions::Answer::Refused,
                QuestionKind::Predicate | QuestionKind::Choice(_) | QuestionKind::Score(_),
            ) => Answer::Refused,
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
            (
                decisions::Answer::Predicate(_)
                | decisions::Answer::Choice(_)
                | decisions::Answer::Score(_),
                QuestionKind::Predicate | QuestionKind::Choice(_) | QuestionKind::Score(_),
            ) => {
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
