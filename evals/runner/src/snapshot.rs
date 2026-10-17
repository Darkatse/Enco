use crate::{
    enco::{Enco, output::*},
    result,
};
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

#[derive(Default)]
pub(crate) struct Metrics {
    pub(crate) rounds: u64,
    pub(crate) usage: result::Usage,
    pub(crate) generations: BTreeMap<String, u64>,
}

#[derive(Default)]
pub(crate) struct Snapshot {
    pub(crate) metrics: Metrics,
    pub(crate) failures: Vec<String>,
    pub(crate) export_errors: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Proposal {
    round: String,
    call: String,
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct Call {
    session: String,
    #[serde(flatten)]
    proposal: Proposal,
    outcome: CallOutcome,
    code: Option<String>,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CallOutcome {
    Ok,
    Failed,
    Unknown,
}

pub(crate) async fn export(enco: &Enco, directory: &Path, forbidden: &[&Path]) -> Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    for name in ["schedules", "memory"] {
        snapshot
            .read::<Value>(enco, directory, name, &[name])
            .await?;
    }
    if let Some(status) = snapshot
        .read::<Status>(enco, directory, "status", &["status", "--json"])
        .await?
    {
        for plugin in status.plugins.into_values().filter_map(|unit| unit.running) {
            match plugin.running {
                Running::Serving { name, activation }
                | Running::Unavailable { name, activation } => {
                    snapshot
                        .metrics
                        .generations
                        .insert(name, activation.generation.id);
                }
                Running::Stopped => {}
            }
        }
    }
    let mut calls = Vec::new();
    if let Some(sessions) = snapshot
        .read::<Vec<Session>>(enco, directory, "sessions", &["sessions"])
        .await?
    {
        for session in sessions {
            let name = format!("log/{}", session.name);
            if let Some(log) = snapshot
                .read::<Vec<Entry>>(enco, directory, &name, &["log", "--session", &session.name])
                .await?
            {
                match snapshot.project(&session.name, log, &mut calls) {
                    Ok(transcript) => {
                        let path = directory.join("transcript");
                        tokio::fs::create_dir_all(&path).await?;
                        tokio::fs::write(path.join(format!("{}.md", session.name)), transcript)
                            .await?;
                    }
                    Err(error) => {
                        snapshot.export_errors.insert(name, format!("{error:#}"));
                    }
                }
            }
        }
    }
    for call in &calls {
        if forbidden
            .iter()
            .any(|path| references(&call.proposal.arguments, path))
        {
            snapshot.failures.push(format!(
                "{}: {} arguments reference the evaluator repository or run directory",
                call.session, call.proposal.name
            ));
        }
    }
    result::write_json(&directory.join("tool-calls.json"), &calls).await?;
    Ok(snapshot)
}

impl Snapshot {
    async fn read<T: DeserializeOwned>(
        &mut self,
        enco: &Enco,
        directory: &Path,
        name: &str,
        args: &[&str],
    ) -> Result<Option<T>> {
        let decoded = match enco.command(args).await {
            Ok(value) => {
                result::write_json(&directory.join(format!("{name}.json")), &value).await?;
                serde_json::from_value(value).context("CLI output does not match its contract")
            }
            Err(error) => Err(error),
        };
        match decoded {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                self.export_errors.insert(name.into(), format!("{error:#}"));
                Ok(None)
            }
        }
    }

    fn project(&mut self, session: &str, log: Vec<Entry>, calls: &mut Vec<Call>) -> Result<String> {
        let mut transcript = String::new();
        let mut attempts = BTreeMap::new();
        let mut proposals = Vec::new();
        let mut settlements = BTreeMap::new();
        for entry in log {
            match entry.body {
                EntryBody::EventConsumed { event } => {
                    let (role, text) = match event.body {
                        EventBody::UserMessage { text } => ("owner", text),
                        EventBody::Reminder { text } => ("reminder", text),
                        EventBody::Other => continue,
                    };
                    transcript.push_str(&format!("## {role}\n{text}\n\n"));
                }
                EntryBody::RoundStarted => self.metrics.rounds += 1,
                EntryBody::AttemptStarted {
                    attempt,
                    purpose,
                    round,
                } => {
                    attempts.insert(attempt, (purpose, round));
                }
                EntryBody::AttemptSettled {
                    attempt,
                    result: AttemptResult::Completed { message, usage },
                } => {
                    self.metrics.usage.input_tokens += usage.input_tokens;
                    self.metrics.usage.output_tokens += usage.output_tokens;
                    self.metrics.usage.cached_input_tokens = self
                        .metrics
                        .usage
                        .cached_input_tokens
                        .zip(usage.cached_input_tokens)
                        .map(|(a, b)| a + b);
                    let (purpose, round) = attempts
                        .get(&attempt)
                        .context("settled Attempt was not started")?;
                    if !matches!(purpose, Purpose::Reply) {
                        continue;
                    }
                    let mut text = Vec::new();
                    for part in message.parts {
                        match part {
                            Part::Text { text: part } => text.push(part),
                            Part::ToolCall {
                                id,
                                name,
                                arguments,
                            } => proposals.push(Proposal {
                                round: round.clone(),
                                call: id,
                                name,
                                arguments,
                            }),
                            Part::Other => {}
                        }
                    }
                    if !text.is_empty() {
                        transcript.push_str(&format!("## assistant\n{}\n\n", text.join("\n")));
                    }
                }
                EntryBody::ToolCallSettled {
                    call,
                    outcome,
                    content,
                } => {
                    settlements.insert(call, (outcome, content));
                }
                EntryBody::RunEnded {
                    end: RunEnd::Completed,
                } => {}
                EntryBody::RunEnded { end } => {
                    self.failures.push(format!(
                        "{session}: Run ended with {}",
                        serde_json::to_string(&end)?
                    ));
                }
                _ => {}
            }
        }
        pair(session, proposals, settlements, calls)?;
        Ok(transcript)
    }
}

/// Every tool call a Reply proposed has exactly one settlement, and every settlement a proposal.
fn pair(
    session: &str,
    proposals: Vec<Proposal>,
    mut settlements: BTreeMap<String, (Settlement, String)>,
    calls: &mut Vec<Call>,
) -> Result<()> {
    for proposal in proposals {
        let (settlement, content) = settlements
            .remove(&proposal.call)
            .with_context(|| format!("{session}: unsettled tool call {}", proposal.call))?;
        let (outcome, code) = match settlement {
            Settlement::Ok => (CallOutcome::Ok, None),
            Settlement::Failed { failure } => (CallOutcome::Failed, Some(failure.code)),
            Settlement::Unknown { failure } => (CallOutcome::Unknown, Some(failure.code)),
        };
        calls.push(Call {
            session: session.into(),
            proposal,
            outcome,
            code,
            content,
        });
    }
    ensure!(
        settlements.is_empty(),
        "{session}: tool settlement without proposal"
    );
    Ok(())
}

fn references(arguments: &str, path: &Path) -> bool {
    let path = path.to_string_lossy();
    let component = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~');
    arguments.match_indices(path.as_ref()).any(|(start, _)| {
        let before = arguments[..start].chars().next_back();
        let after = arguments[start + path.len()..].chars().next();
        before.is_none_or(|c| !component(c)) && after.is_none_or(|c| c == '/' || !component(c))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_checks_match_complete_paths() {
        for (arguments, expected) in [
            (r#"{"command":"cat /tmp/lin/.enco/workspace/note"}"#, false),
            (r#"{"command":"cat /work-copy/note"}"#, false),
            (r#"{"command":"cat /work/note"}"#, true),
        ] {
            assert_eq!(
                references(arguments, Path::new("/work")),
                expected,
                "{arguments}"
            );
        }
    }
}
