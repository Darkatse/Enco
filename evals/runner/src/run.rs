use crate::{
    Options,
    config::{Config, Model, Variant},
    enco::Binary,
    grade::Grading,
    judge,
    render::Renderer,
    result::{
        DeployRecord, Execution, Facts, JudgeRecord, Report, TaskRecord, TrialOutcome,
        VariantRecord,
    },
    stats,
    task::{self, Task},
    trial::{self, TrialDeps},
};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::JoinSet;

pub(crate) async fn directory(repository: &Path) -> Result<PathBuf> {
    let suffix = ulid::Ulid::generate().to_string()[22..].to_ascii_lowercase();
    let root = repository.join("target/evals");
    tokio::fs::create_dir_all(&root).await?;
    let path = root.join(format!("{}-{suffix}", Utc::now().format("%Y%m%d-%H%M%S")));
    tokio::fs::create_dir(&path).await?;
    Ok(path)
}

#[expect(
    clippy::print_stdout,
    reason = "the evaluation CLI reports progress and the result path"
)]
pub(crate) async fn evaluate(
    repository: &Path,
    options: Options,
    variants: Vec<(&str, Option<PathBuf>)>,
    targets: Option<Vec<String>>,
) -> Result<()> {
    ensure!(options.k > 0, "k must be positive");
    let config = Config::read(&options.config, repository).await?;
    let model = Model::read(&config.model).await?;
    let mut tasks = Vec::new();
    for path in task::select(repository, &options.tasks).await? {
        tasks.push(Arc::new(Task::read(path).await?));
    }
    if let Some(targets) = &targets {
        ensure!(
            targets
                .iter()
                .all(|id| tasks.iter().any(|task| task.id == *id)),
            "every target must be one of the selected tasks"
        );
    }
    let mut prepared = Vec::new();
    let mut identities = BTreeMap::new();
    for (name, path) in variants {
        let variant = Variant::read(path.as_deref()).await?;
        let binary = Binary::resolve(&variant).await?;
        identities.insert(name.to_owned(), identity(&binary, &variant).await?);
        prepared.push(Arc::new(trial::Variant {
            name: name.into(),
            binary,
            deploy: variant.deploy,
        }));
    }
    let judge = judge::Client::new(config.judge.clone())?;
    let directory = directory(repository).await?;
    let started_at = Utc::now();
    let task_records = tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                TaskRecord {
                    hash: task.hash.clone(),
                },
            )
        })
        .collect();
    let model_identity = model.identity.clone();
    let judge_record = JudgeRecord {
        config: config.judge.clone(),
        prompt: judge::prompt_hash(),
    };
    println!("results: {}", directory.join("result.json").display());
    let deps = Arc::new(TrialDeps {
        repository: repository.to_owned(),
        directory,
        config,
        model: Some(model),
        judge,
    });
    let mut queue = VecDeque::new();
    for task in tasks {
        for index in 0..options.k {
            for variant in &prepared {
                queue.push_back((task.clone(), variant.clone(), index));
            }
        }
    }
    let mut running = JoinSet::new();
    let mut failure = None;
    let mut trials = Vec::new();
    while !queue.is_empty() || !running.is_empty() {
        while running.len() < options.jobs.get() && failure.is_none() {
            let Some((task, variant, index)) = queue.pop_front() else {
                break;
            };
            let deps = deps.clone();
            running.spawn(async move { trial::run(&deps, &task, &variant, index).await });
        }
        let Some(completed) = running.join_next().await else {
            break;
        };
        match completed.context("trial task").and_then(|result| result) {
            Ok(trial) => {
                let outcome = match trial.result {
                    TrialOutcome::Success => "success",
                    TrialOutcome::Failure => "failure",
                    TrialOutcome::InstrumentError => "instrument_error",
                };
                println!(
                    "{}/{}/{}: {outcome}",
                    trial.task, trial.variant, trial.index
                );
                trials.push(trial);
            }
            Err(error) => {
                failure.get_or_insert(error);
                queue.clear();
            }
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    trials.sort_by(|a, b| (&a.task, &a.index, &a.variant).cmp(&(&b.task, &b.index, &b.variant)));
    let mut report = Report {
        format: 1,
        runner: env!("CARGO_PKG_VERSION").into(),
        started_at,
        finished_at: Utc::now(),
        k: options.k,
        tasks: task_records,
        model: model_identity,
        judge: judge_record,
        variants: identities,
        trials,
        summary: BTreeMap::new(),
        comparison: None,
    };
    report.summarize();
    if let Some(targets) = targets {
        report.comparison = Some(stats::compare(&report, &targets)?);
    }
    crate::result::write_json(&deps.directory.join("result.json"), &report).await
}

async fn identity(binary: &Binary, variant: &Variant) -> Result<VariantRecord> {
    let mut deploy = Vec::new();
    for item in &variant.deploy {
        let bytes = tokio::fs::read(&item.artifact).await?;
        deploy.push(DeployRecord {
            plugin: item.plugin.clone(),
            artifact: item.artifact.clone(),
            hash: blake3::hash(&bytes).to_hex().to_string(),
        });
    }
    Ok(VariantRecord {
        enco: binary.clone(),
        deploy,
    })
}

pub(crate) async fn regrade(repository: &Path, directory: &Path, fresh_judge: bool) -> Result<()> {
    let directory = tokio::fs::canonicalize(directory).await?;
    let path = directory.join("result.json");
    let mut report: Report = crate::result::read_json(&path).await?;
    let ids: Vec<_> = report.tasks.keys().cloned().collect();
    let mut tasks = BTreeMap::new();
    for path in task::select(repository, &ids).await? {
        let task = Task::read(path).await?;
        ensure!(
            report
                .tasks
                .get(&task.id)
                .is_some_and(|saved| saved.hash == task.hash),
            "{}: task hash differs from the recorded evaluation",
            task.id
        );
        tasks.insert(task.id.clone(), task);
    }
    let judge = fresh_judge
        .then(|| judge::Client::new(report.judge.config.clone()))
        .transpose()?;
    for trial in &mut report.trials {
        let task = tasks
            .get(&trial.task)
            .context("trial refers to an unknown task")?;
        let directory = directory.join(&trial.dir);
        let facts: Facts = crate::result::read_json(&directory.join("trial.json")).await?;
        let grading = Grading {
            task,
            snapshot: &directory.join("snapshot"),
            trial: &directory,
            output: &directory,
            renderer: Renderer {
                start: facts.start,
                timezone: task.definition.environment.timezone,
                mocks: task.mock_urls(),
            },
            judge: judge.as_ref(),
        };
        let criteria = if matches!(facts.execution, Execution::InstrumentError { .. }) {
            BTreeMap::new()
        } else {
            grading.all(None).await
        };
        crate::result::write_json(&directory.join("grades.json"), &criteria).await?;
        trial.graded(&facts, criteria);
    }
    if fresh_judge {
        report.judge.prompt = judge::prompt_hash();
    }
    report.summarize();
    if let Some(comparison) = &report.comparison {
        report.comparison = Some(stats::compare(&report, &comparison.targets)?);
    }
    crate::result::write_json(&path, &report).await
}
