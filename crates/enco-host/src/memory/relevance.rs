use super::{MemoryError, reported_call};
use crate::limits::MEMORY_RELEVANCE_THRESHOLD;
use enco_core::{Memory, Omission, ProviderSettings};
use enco_kernel::{Answer, Question, QuestionKind, Registry};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const RELEVANCE_QUESTION: &str = "Would this memory help in responding to the message?";

/// Configured decision caller; the adapter is resolved for each evaluation.
#[derive(Clone)]
pub struct DecisionEndpoint {
    pub plugin: String,
    pub settings: ProviderSettings,
    pub api_key: Option<String>,
}

/// Optional filtering of newly recalled memories before they become context candidates.
pub struct Relevance {
    endpoint: DecisionEndpoint,
    registry: Arc<Registry>,
}

pub(super) struct Filtered {
    pub kept: Vec<Memory>,
    pub omitted: Vec<Omission>,
}

impl Relevance {
    pub fn new(endpoint: DecisionEndpoint, registry: Arc<Registry>) -> Self {
        Self { endpoint, registry }
    }

    pub(super) async fn filter(
        &self,
        query: &str,
        recalled: Vec<Memory>,
        cancel: &CancellationToken,
    ) -> Result<Filtered, MemoryError> {
        if recalled.is_empty() {
            return Ok(Filtered {
                kept: recalled,
                omitted: vec![],
            });
        }
        let questions = recalled
            .iter()
            .map(|memory| Question {
                instructions: format!("{RELEVANCE_QUESTION}\n\nMemory: {}", memory.text),
                kind: QuestionKind::Predicate,
            })
            .collect();
        let endpoint = &self.endpoint;
        let result = reported_call(
            &self.registry,
            self.registry.exports().decision(&endpoint.plugin),
            cancel,
            |adapter| async move {
                adapter
                    .decide(
                        &endpoint.settings,
                        endpoint.api_key.as_deref(),
                        query.to_owned(),
                        questions,
                    )
                    .await
            },
        )
        .await?;
        let answers = match result {
            Ok(answers) => answers,
            Err(failure) => {
                return Ok(Filtered {
                    kept: recalled,
                    omitted: vec![Omission {
                        source: "memory:relevance".into(),
                        reason: format!(
                            "decision failed ({}): {}; recalled memories were not filtered",
                            failure.code, failure.message
                        ),
                    }],
                });
            }
        };
        let mut kept = Vec::new();
        let mut omitted = Vec::new();
        let mut refused = 0;
        for (memory, answer) in recalled.into_iter().zip(answers) {
            match answer {
                Answer::Predicate(p) if p < MEMORY_RELEVANCE_THRESHOLD => omitted.push(Omission {
                    source: format!("memory:{}", memory.id),
                    reason: format!("judged irrelevant to the input (p={p:.2})"),
                }),
                Answer::Refused => {
                    refused += 1;
                    kept.push(memory);
                }
                _ => kept.push(memory),
            }
        }
        if refused > 0 {
            omitted.push(Omission {
                source: "memory:relevance".into(),
                reason: format!("decision refused {refused} memories; they were kept"),
            });
        }
        Ok(Filtered { kept, omitted })
    }
}
