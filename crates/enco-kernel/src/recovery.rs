use crate::dispatch::render_text;
use enco_core::*;
use std::collections::{HashMap, HashSet};

pub(crate) fn recover(entries: &[Entry]) -> Vec<EntryBody> {
    let Some((start, run)) = entries
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match e.body {
            EntryBody::RunStarted { run } => Some((i, run)),
            _ => None,
        })
    else {
        return vec![];
    };
    let tail = &entries[start..];
    if tail.iter().any(|entry| {
        matches!(
            entry.body,
            EntryBody::RunEnded { run: ended_run, .. } if ended_run == run
        )
    }) {
        return vec![];
    }
    let mut result = vec![];
    if let Some((index, round)) = tail
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match e.body {
            EntryBody::RoundStarted { round, .. } => Some((i, round)),
            _ => None,
        })
    {
        let round_entries = &tail[index..];
        if !round_entries.iter().any(|entry| {
            matches!(
                entry.body,
                EntryBody::RoundEnded { round: ended_round, .. } if ended_round == round
            )
        }) {
            settle_round(round_entries, &mut result);
            result.push(EntryBody::RoundEnded {
                round,
                end: RoundEnd::Interrupted,
            });
        }
    }
    result.push(EntryBody::RunEnded {
        run,
        end: RunEnd::Interrupted,
    });
    result
}

fn settle_round(entries: &[Entry], result: &mut Vec<EntryBody>) {
    let mut attempts = HashMap::new();
    let mut settled_attempts = HashSet::new();
    let mut started_calls = HashMap::new();
    let mut settled_calls = HashSet::new();
    let mut calls = vec![];
    for e in entries {
        match &e.body {
            EntryBody::AttemptStarted {
                attempt, purpose, ..
            } => {
                attempts.insert(*attempt, *purpose);
            }
            EntryBody::AttemptSettled { attempt, result } => {
                settled_attempts.insert(*attempt);
                if let AttemptResult::Completed { message, .. } = result
                    && attempts.get(attempt) == Some(&AttemptPurpose::Reply)
                {
                    calls = message.tool_calls().cloned().collect();
                }
            }
            EntryBody::ToolCallStarted { call, effect, .. } => {
                started_calls.insert(*call, *effect);
            }
            EntryBody::ToolCallSettled { call, .. } => {
                settled_calls.insert(*call);
            }
            _ => {}
        }
    }
    // Preserve Log order rather than HashMap iteration order when closing attempts.
    for e in entries {
        if let EntryBody::AttemptStarted { attempt, .. } = e.body
            && !settled_attempts.contains(&attempt)
        {
            result.push(EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Failed {
                    failure: interrupted(true),
                },
            });
        }
    }
    for call in calls {
        if settled_calls.contains(&call.id) {
            continue;
        }
        let outcome = match started_calls.get(&call.id) {
            Some(Effect::ReadOnly) => Outcome::Failed {
                failure: interrupted(true),
            },
            Some(_) => Outcome::Unknown {
                failure: interrupted(false),
            },
            None => Outcome::Failed {
                failure: Failure {
                    code: code::NOT_DISPATCHED.into(),
                    message: "not executed: the process stopped before this call was dispatched"
                        .into(),
                    retryable: false,
                },
            },
        };
        let content = render_text(&outcome);
        result.push(EntryBody::ToolCallSettled {
            call: call.id,
            outcome,
            content,
            full: None,
        });
    }
}

fn interrupted(retryable: bool) -> Failure {
    Failure {
        code: code::INTERRUPTED.into(),
        message: "process stopped before this operation was settled".into(),
        retryable,
    }
}
