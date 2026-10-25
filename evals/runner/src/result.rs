use crate::{
    config::{Judge, ModelRecord},
    enco::Binary,
    grade::{Grade, Kind},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
pub(crate) struct Report {
    pub(crate) format: u32,
    pub(crate) runner: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: DateTime<Utc>,
    pub(crate) k: usize,
    pub(crate) tasks: BTreeMap<String, TaskRecord>,
    pub(crate) model: ModelRecord,
    pub(crate) judge: JudgeRecord,
    pub(crate) variants: BTreeMap<String, VariantRecord>,
    pub(crate) trials: Vec<Trial>,
    pub(crate) summary: BTreeMap<String, BTreeMap<String, Summary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) comparison: Option<crate::stats::Comparison>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct VariantRecord {
    pub(crate) enco: Binary,
    pub(crate) deploy: Vec<DeployRecord>,
    pub(crate) plugin: toml::Table,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DeployRecord {
    pub(crate) plugin: String,
    pub(crate) artifact: PathBuf,
    pub(crate) hash: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TaskRecord {
    pub(crate) hash: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct JudgeRecord {
    #[serde(flatten)]
    pub(crate) config: Judge,
    pub(crate) prompt: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrialOutcome {
    Success,
    Failure,
    InstrumentError,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Trial {
    pub(crate) task: String,
    pub(crate) variant: String,
    pub(crate) index: usize,
    pub(crate) dir: PathBuf,
    pub(crate) start: DateTime<Utc>,
    pub(crate) result: TrialOutcome,
    pub(crate) reason: String,
    pub(crate) criteria: BTreeMap<String, Grade>,
    pub(crate) rounds: u64,
    pub(crate) wall_secs: f64,
    pub(crate) usage: Usage,
    pub(crate) generations: BTreeMap<String, u64>,
    pub(crate) network_violations: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Usage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cached_input_tokens: Option<u64>,
}

/// The sum over no Attempts: every one of them reported its cached tokens.
impl Default for Usage {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: Some(0),
        }
    }
}

/// Execution facts that regrading never changes.
#[derive(Serialize, Deserialize)]
pub(crate) struct Facts {
    pub(crate) start: DateTime<Utc>,
    pub(crate) home: PathBuf,
    pub(crate) execution: Execution,
    pub(crate) export_errors: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub(crate) enum Execution {
    Completed,
    Failure { reason: String },
    InstrumentError { reason: String },
}

impl Execution {
    pub(crate) fn failed(error: impl std::fmt::Display) -> Self {
        Self::Failure {
            reason: format!("{error:#}"),
        }
    }

    /// Adds failures the subject showed, keeping every reason so that none hides behind the first.
    pub(crate) fn fail_with(&mut self, reasons: &[String]) {
        let mut all = match self {
            Self::Completed => Vec::new(),
            Self::Failure { reason } => vec![reason.clone()],
            Self::InstrumentError { .. } => return,
        };
        all.extend(reasons.iter().cloned());
        if !all.is_empty() {
            *self = Self::Failure {
                reason: all.join("; "),
            };
        }
    }

    /// Records an instrument failure without hiding a failure the subject already showed.
    pub(crate) fn mark_instrument_error(&mut self, error: impl std::fmt::Display) {
        if matches!(self, Self::Completed) {
            *self = Self::InstrumentError {
                reason: format!("{error:#}"),
            };
        }
    }
}

impl Trial {
    pub(crate) fn graded(&mut self, facts: &Facts, criteria: BTreeMap<String, Grade>) {
        (self.result, self.reason) = outcome(&facts.execution, &criteria);
        self.criteria = criteria;
    }
}

pub(crate) fn outcome(
    execution: &Execution,
    criteria: &BTreeMap<String, Grade>,
) -> (TrialOutcome, String) {
    let failed: Vec<_> = criteria
        .iter()
        .filter(|(_, grade)| grade.result == Kind::Fail)
        .map(|(id, _)| id.as_str())
        .collect();
    let errors: Vec<_> = criteria
        .iter()
        .filter(|(_, grade)| grade.result == Kind::Error)
        .map(|(id, _)| id.as_str())
        .collect();
    if let Execution::Failure { reason } = execution {
        (TrialOutcome::Failure, reason.clone())
    } else if !failed.is_empty() {
        (
            TrialOutcome::Failure,
            format!("failed criteria: {}", failed.join(", ")),
        )
    } else if let Execution::InstrumentError { reason } = execution {
        (TrialOutcome::InstrumentError, reason.clone())
    } else if !errors.is_empty() {
        (
            TrialOutcome::InstrumentError,
            format!("grading errors: {}", errors.join(", ")),
        )
    } else {
        (TrialOutcome::Success, "all criteria passed".into())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Summary {
    pub(crate) passes: usize,
    pub(crate) valid: usize,
    pub(crate) instrument_errors: usize,
    pub(crate) all_passed: bool,
}

impl Report {
    pub(crate) fn summarize(&mut self) {
        self.summary.clear();
        for trial in &self.trials {
            let summary = self
                .summary
                .entry(trial.variant.clone())
                .or_default()
                .entry(trial.task.clone())
                .or_default();
            match trial.result {
                TrialOutcome::Success => {
                    summary.passes += 1;
                    summary.valid += 1;
                }
                TrialOutcome::Failure => summary.valid += 1,
                TrialOutcome::InstrumentError => summary.instrument_errors += 1,
            }
            summary.all_passed = summary.passes == self.k;
        }
    }
}

pub(crate) async fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let pending = path.with_extension("json.tmp");
    tokio::fs::write(&pending, serde_json::to_vec_pretty(value)?)
        .await
        .with_context(|| format!("write {}", path.display()))?;
    tokio::fs::rename(pending, path)
        .await
        .with_context(|| format!("publish {}", path.display()))
}

pub(crate) async fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_failure_does_not_hide_behind_the_first() {
        let mut execution = Execution::Completed;
        execution.fail_with(&[
            "main: Run ended with budget_exhausted".into(),
            "main: shell_exec arguments reference the evaluator repository".into(),
        ]);
        assert!(matches!(
            execution,
            Execution::Failure { reason } if reason.contains("budget_exhausted")
                && reason.contains("evaluator repository")
        ));
    }
}
