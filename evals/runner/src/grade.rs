use crate::{
    judge,
    limits::{GRADER_TIMEOUT, REASON_BYTES},
    render::Renderer,
    task::{self, Criterion, Grader, Task},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, process::Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Pass,
    Fail,
    Error,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Grade {
    pub(crate) result: Kind,
    pub(crate) reason: String,
}

impl Grade {
    pub(crate) fn new(result: Kind, reason: impl Into<String>) -> Self {
        let mut reason = reason.into();
        reason.truncate(reason.floor_char_boundary(REASON_BYTES));
        Self { result, reason }
    }
}

pub(crate) struct Grading<'a> {
    pub(crate) task: &'a Task,
    pub(crate) snapshot: &'a Path,
    pub(crate) trial: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) renderer: Renderer,
    /// None reuses the saved judge replies without making a request.
    pub(crate) judge: Option<&'a judge::Client>,
}

impl Grading<'_> {
    pub(crate) async fn all(
        &self,
        selected: Option<&std::collections::BTreeSet<String>>,
    ) -> BTreeMap<String, Grade> {
        let mut grades = BTreeMap::new();
        for criterion in &self.task.definition.criteria {
            if selected.is_some_and(|ids| !ids.contains(&criterion.id)) {
                continue;
            }
            let grade = self
                .one(criterion)
                .await
                .unwrap_or_else(|error| Grade::new(Kind::Error, format!("{error:#}")));
            grades.insert(criterion.id.clone(), grade);
        }
        grades
    }

    async fn one(&self, criterion: &Criterion) -> Result<Grade> {
        match &criterion.grader {
            Grader::State { snapshot, schema } => {
                let source = tokio::fs::read_to_string(self.task.root.join(schema)).await?;
                let schema: Value = serde_json::from_str(&self.renderer.text(&source)?)?;
                let validator = jsonschema::options()
                    .with_draft(jsonschema::Draft::Draft202012)
                    .build(&schema)?;
                let path = self.snapshot.join(snapshot);
                if !path.try_exists()? {
                    return Ok(Grade::new(
                        Kind::Fail,
                        format!("missing snapshot {}", snapshot.display()),
                    ));
                }
                let instance: Value = crate::result::read_json(&path).await?;
                let errors: Vec<_> = validator
                    .iter_errors(&instance)
                    .take(5)
                    .map(|error| error.to_string())
                    .collect();
                Ok(if errors.is_empty() {
                    Grade::new(Kind::Pass, "schema matched")
                } else {
                    Grade::new(Kind::Fail, errors.join("; "))
                })
            }
            Grader::Rubric {
                sessions,
                evidence,
                reference,
            } => {
                let path = self
                    .output
                    .join("judge")
                    .join(format!("{}.json", criterion.id));
                let Some(prompt) = self
                    .evidence(criterion, sessions.as_deref(), evidence, reference)
                    .await?
                else {
                    return Ok(Grade::new(Kind::Fail, "required evidence was not produced"));
                };
                let Some(judge) = self.judge else {
                    let record: judge::Record = crate::result::read_json(&path).await?;
                    return Ok(record.grade());
                };
                judge.grade(prompt, &path).await
            }
            Grader::Command { run } => self.command(run, &criterion.id).await,
        }
    }

    async fn evidence(
        &self,
        criterion: &Criterion,
        sessions: Option<&[String]>,
        evidence: &[String],
        reference: &str,
    ) -> Result<Option<String>> {
        let mut prompt = format!(
            "## Criterion\n{}\n\n## Reference\n{}\n",
            criterion.check,
            self.renderer.text(reference)?
        );
        let transcript = self.snapshot.join("transcript");
        let names = match sessions {
            Some(names) => names.to_vec(),
            None if transcript.try_exists()? => task::files(&transcript)
                .await?
                .iter()
                .filter_map(|path| {
                    path.file_stem()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                })
                .collect(),
            None => return Ok(None),
        };
        for name in names {
            let path = transcript.join(format!("{name}.md"));
            if !path.try_exists()? {
                return Ok(None);
            }
            prompt.push_str(&format!(
                "\n## Transcript: {name}\n{}\n",
                tokio::fs::read_to_string(path).await?
            ));
        }
        for evidence in evidence {
            let (path, pointer) = evidence
                .split_once('#')
                .map_or((evidence.as_str(), None), |(path, pointer)| {
                    (path, Some(pointer))
                });
            let path = self.snapshot.join(path);
            if !path.try_exists()? {
                return Ok(None);
            }
            let value: Value = crate::result::read_json(&path).await?;
            let value = match pointer {
                Some(pointer) => match value.pointer(pointer) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                None => &value,
            };
            prompt.push_str(&format!(
                "\n## Evidence: {evidence}\n{}\n",
                serde_json::to_string_pretty(value)?
            ));
        }
        Ok(Some(prompt))
    }

    async fn command(&self, run: &Path, id: &str) -> Result<Grade> {
        let directory = self.output.join("command");
        tokio::fs::create_dir_all(&directory).await?;
        let stdout = directory.join(format!("{id}.stdout"));
        let stderr = directory.join(format!("{id}.stderr"));
        let mut child = tokio::process::Command::new(self.task.root.join(run))
            .arg(self.trial)
            .current_dir(&self.task.root)
            .stdin(Stdio::null())
            .stdout(tokio::fs::File::create(&stdout).await?.into_std().await)
            .stderr(tokio::fs::File::create(&stderr).await?.into_std().await)
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("grader {}", run.display()))?;
        let status = match tokio::time::timeout(GRADER_TIMEOUT, child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                child.kill().await?;
                return Ok(Grade::new(Kind::Error, "command grader timed out"));
            }
        };
        let kind = match status.code() {
            Some(0) => Kind::Pass,
            Some(1) => Kind::Fail,
            _ => Kind::Error,
        };
        // A grader script ends with its verdict.
        let prefix = format!("{status}: ");
        let output = tokio::fs::read_to_string(stdout).await?;
        let keep = REASON_BYTES.saturating_sub(prefix.len());
        let tail = &output[output.ceil_char_boundary(output.len().saturating_sub(keep))..];
        Ok(Grade::new(kind, prefix + tail))
    }
}
