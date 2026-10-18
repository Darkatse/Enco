use super::*;
use crate::{ProviderRequest, Question, QuestionKind, limits::*};

/// A new use is checked against its candidate code, before that code receives a generation ID.
pub(super) async fn call(loaded: &Loaded, usage: &Use) -> Result<Probed, Failure> {
    match usage.need {
        Need::Completion {
            tools,
            max_output_tokens,
        } => {
            let provider =
                usage
                    .endpoint
                    .export(loaded, loaded.completion.as_ref(), "completion")?;
            let result = provider
                .complete(
                    &usage.endpoint.settings,
                    ProviderRequest {
                        messages: vec![Message::text(Role::User, CONFIG_PROBE_PROMPT)],
                        tools: if tools {
                            vec![CONFIG_PROBE_TOOL.clone()]
                        } else {
                            vec![]
                        },
                        max_output_tokens: Some(max_output_tokens.get()),
                    },
                )
                .await;
            Ok(checked(result, |_| Ok(())))
        }
        Need::Embedding { dimensions } => {
            let provider = usage
                .endpoint
                .export(loaded, loaded.embedding.as_ref(), "embedding")?;
            let result = provider
                .embed(&usage.endpoint.settings, vec![CONFIG_PROBE_PROMPT.into()])
                .await;
            Ok(checked(result, |vectors| {
                if vectors.len() == 1 && vectors[0].len() == dimensions.get() {
                    Ok(())
                } else {
                    Err(Failure {
                        code: code::PROVIDER_BAD_RESPONSE.into(),
                        message: format!("expected one embedding of width {dimensions}"),
                        retryable: false,
                    })
                }
            }))
        }
        Need::Decision => {
            let provider = usage
                .endpoint
                .export(loaded, loaded.decision.as_ref(), "decision")?;
            let result = provider
                .decide(
                    &usage.endpoint.settings,
                    CONFIG_PROBE_PROMPT.into(),
                    vec![Question {
                        instructions: CONFIG_PROBE_QUESTION.into(),
                        kind: QuestionKind::Predicate,
                    }],
                )
                .await;
            Ok(checked(result, |_| Ok(())))
        }
    }
}

pub(super) struct Probed {
    pub outcome: Result<(), Failure>,
    pub verdict: Verdict,
}

/// Health observes the invocation before the caller checks its own requirements.
fn checked<T>(outcome: Result<T, Failure>, check: impl FnOnce(T) -> Result<(), Failure>) -> Probed {
    let verdict = match &outcome {
        Ok(_) => Verdict::Ok,
        Err(failure) => Verdict::Failed(failure.clone()),
    };
    Probed {
        outcome: outcome.and_then(check),
        verdict,
    }
}
