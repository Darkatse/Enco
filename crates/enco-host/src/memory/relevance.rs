use super::{Memories, MemoryError, reported_call};
use crate::limits::MEMORY_RELEVANCE_THRESHOLD;
use enco_core::{Memory, Omission};
use enco_kernel::{Answer, Question, QuestionKind};
use tokio_util::sync::CancellationToken;

/// The yes-or-no question asked about each recalled memory to decide whether to keep it.
///
/// It asks for relatedness rather than usefulness: dropping a needed memory costs more than
/// keeping a marginal one, so only memories about something unrelated should fall below the
/// threshold.
const RELEVANCE_QUESTION: &str = "Does this stored memory about the user relate to the message?\n\
    Yes if it concerns the message's topic, request, people or things; \
    no if it is about something unrelated.";

pub(super) struct Filtered {
    pub kept: Vec<Memory>,
    pub omitted: Vec<Omission>,
}

impl Memories {
    /// Filter automatic recall with the current memory settings.
    pub(super) async fn filter_relevance(
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
        let view = self.registry.view();
        let settings = match self.settings.get(&view) {
            Ok(settings) => settings,
            Err(error) => {
                return Ok(Filtered {
                    kept: recalled,
                    omitted: vec![Omission {
                        source: "memory:relevance".into(),
                        reason: format!(
                            "config.invalid: {error}; recalled memories were not filtered"
                        ),
                    }],
                });
            }
        };
        let Some(endpoint) = &settings.relevance else {
            return Ok(Filtered {
                kept: recalled,
                omitted: vec![],
            });
        };
        let questions = recalled
            .iter()
            .map(|memory| Question {
                instructions: format!("{RELEVANCE_QUESTION}\n\nMemory: {}", memory.text),
                kind: QuestionKind::Predicate,
            })
            .collect();
        let result = reported_call(
            &self.registry,
            view.exports.decision(endpoint),
            cancel,
            |adapter| async move {
                adapter
                    .decide(&endpoint.settings, query.to_owned(), questions)
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
