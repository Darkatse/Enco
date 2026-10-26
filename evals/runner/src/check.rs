#![expect(
    clippy::print_stdout,
    reason = "the check command reports calibration results"
)]
use crate::{
    config::{Config, Variant},
    enco::Binary,
    grade::{Grading, Kind},
    judge,
    render::Renderer,
    result::{self, Execution, Facts, TrialOutcome},
    run,
    task::{self, Expectation, Grader, Task},
    trial::{self, TrialDeps},
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use std::path::Path;

pub(crate) async fn check(repository: &Path, config: &Path, selected: Vec<String>) -> Result<bool> {
    let config = Config::read(config, repository).await?;
    let judge = judge::Client::new(config.judge.clone())?;
    let paths = task::select(repository, &selected).await?;
    let mut prepared = Vec::new();
    let mut passed = true;
    for path in paths {
        let loaded = async {
            let task = Task::read(path.clone()).await?;
            let cases = cases(&task).await?;
            Ok::<_, anyhow::Error>((task, cases))
        }
        .await;
        match loaded {
            Ok(case) => prepared.push(case),
            Err(error) => {
                println!("{}: FAIL: {error:#}", path.display());
                passed = false;
            }
        }
    }
    if prepared.is_empty() {
        return Ok(false);
    }
    let directory = run::directory(repository).await?;
    println!("checks: {}", directory.display());
    let deps = TrialDeps {
        repository: repository.to_owned(),
        directory,
        config,
        model: None,
        judge,
    };
    let base = Variant::read(None).await?;
    let variant = trial::Variant {
        name: "null".into(),
        binary: Binary::resolve(&base).await?,
        source: Vec::new(),
        plugin: base.plugin,
        variables: base.variables,
    };
    for (task, cases) in prepared {
        for (case, expectation, start) in cases {
            passed &= check_case(&deps, &task, &case, &expectation, start).await?;
        }
        passed &= null_run(&deps, &task, &variant).await?;
    }
    Ok(passed)
}

/// Grade one case as its expectation asks; a case with rubric criteria is judged twice, and
/// both judgments must match.
async fn check_case(
    deps: &TrialDeps,
    task: &Task,
    case: &Path,
    expectation: &Expectation,
    start: DateTime<Utc>,
) -> Result<bool> {
    let name = case.file_name().context("case name")?;
    let case_output = deps.directory.join("cases").join(&task.id).join(name);
    let repeats = if task.definition.criteria.iter().any(|criterion| {
        (expectation.success.is_some() || expectation.criteria.contains_key(&criterion.id))
            && matches!(criterion.grader, Grader::Rubric { .. })
    }) {
        2
    } else {
        1
    };
    let selected = expectation.criteria.keys().cloned().collect();
    let mut passed = true;
    for repeat in 0..repeats {
        let output = case_output.join(repeat.to_string());
        let snapshot = case.join("snapshot");
        let grading = Grading {
            task,
            snapshot: &snapshot,
            trial: case,
            output: &output,
            renderer: Renderer {
                start,
                timezone: task.definition.environment.timezone,
                mocks: task.mock_urls(),
            },
            judge: Some(&deps.judge),
        };
        let grades = grading
            .all(expectation.success.is_none().then_some(&selected))
            .await;
        result::write_json(&output.join("grades.json"), &grades).await?;
        let success = grades.values().all(|grade| grade.result == Kind::Pass);
        let matches = expectation
            .success
            .is_none_or(|expected| expected == success)
            && expectation.criteria.iter().all(|(id, expected)| {
                grades
                    .get(id)
                    .is_some_and(|grade| grade.result == *expected)
            })
            && grades.values().all(|grade| grade.result != Kind::Error);
        println!(
            "{}/{} [{}]: {}",
            task.id,
            name.to_string_lossy(),
            repeat + 1,
            if matches { "pass" } else { "FAIL" }
        );
        passed &= matches;
    }
    Ok(passed)
}

/// Run the task against a model that answers everything with one fixed sentence: the daemon
/// must run, and the task's criteria must fail it. A daemon that never ran proves nothing.
async fn null_run(deps: &TrialDeps, task: &Task, variant: &trial::Variant) -> Result<bool> {
    let empty = trial::run(deps, task, variant, 0).await?;
    let facts: Facts =
        result::read_json(&deps.directory.join(&empty.dir).join("trial.json")).await?;
    let failed =
        matches!(facts.execution, Execution::Completed) && empty.result == TrialOutcome::Failure;
    println!(
        "{}/null run: {}",
        task.id,
        if failed {
            "pass".into()
        } else {
            format!("FAIL: {}", empty.reason)
        }
    );
    result::write_json(
        &deps
            .directory
            .join("null")
            .join(format!("{}.json", task.id)),
        &empty,
    )
    .await?;
    Ok(failed)
}

async fn cases(task: &Task) -> Result<Vec<(std::path::PathBuf, Expectation, DateTime<Utc>)>> {
    let root = task.root.join("hidden/cases");
    ensure!(root.is_dir(), "{} has no grading cases", task.id);
    let timed = timed(task).await?;
    let mut cases = Vec::new();
    for path in task::files(&root).await? {
        if path.file_name().is_none_or(|name| name != "expect.toml") {
            continue;
        }
        let expectation: Expectation = task::read_toml(&path).await?;
        ensure!(
            expectation.success.is_some() || !expectation.criteria.is_empty(),
            "{}: no expected outcome",
            path.display()
        );
        let start = case_start(task, &expectation, timed)?;
        cases.push((
            path.parent().context("case directory")?.to_owned(),
            expectation,
            start,
        ));
    }
    ensure!(
        cases
            .iter()
            .any(|(_, expectation, _)| expectation.success == Some(true)),
        "{} needs a passing oracle",
        task.id
    );
    ensure!(
        cases
            .iter()
            .any(|(_, expectation, _)| expectation.success == Some(false)),
        "{} needs a failing null case",
        task.id
    );
    Ok(cases)
}

/// Whether a reference or a schema uses time placeholders, so each case must state its start.
async fn timed(task: &Task) -> Result<bool> {
    let placeholder = |text: &str| text.contains("{{time:") || text.contains("{{date:");
    for criterion in &task.definition.criteria {
        let used = match &criterion.grader {
            Grader::Rubric { reference, .. } => placeholder(reference),
            Grader::State { schema, .. } => {
                placeholder(&tokio::fs::read_to_string(task.root.join(schema)).await?)
            }
            Grader::Command { .. } => false,
        };
        if used {
            return Ok(true);
        }
    }
    Ok(false)
}

fn case_start(task: &Task, expectation: &Expectation, timed: bool) -> Result<DateTime<Utc>> {
    for id in expectation.criteria.keys() {
        ensure!(
            task.definition
                .criteria
                .iter()
                .any(|criterion| criterion.id == *id),
            "unknown criterion {id}"
        );
    }
    ensure!(
        !timed || expectation.start.is_some(),
        "case needs start because references or schemas use time placeholders"
    );
    match &expectation.start {
        Some(start) => Ok(start
            .to_string()
            .parse::<DateTime<chrono::FixedOffset>>()?
            .with_timezone(&Utc)),
        None => Ok(DateTime::UNIX_EPOCH),
    }
}
