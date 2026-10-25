use crate::render::Renderer;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub(crate) category: String,
    pub(crate) mechanism: String,
    pub(crate) environment: Environment,
    pub(crate) input: Vec<Input>,
    pub(crate) budget: Budget,
    pub(crate) criteria: Vec<Criterion>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Environment {
    pub(crate) timezone: chrono_tz::Tz,
    /// Variables the owner has set, such as a service's access token; the daemon inherits them.
    #[serde(default)]
    pub(crate) variables: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    #[serde(default = "main_session")]
    pub(crate) session: String,
    pub(crate) text: String,
}

fn main_session() -> String {
    "main".into()
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    pub(crate) max_rounds: u32,
    pub(crate) timeout_secs: u64,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Criterion {
    pub(crate) id: String,
    pub(crate) check: String,
    #[serde(flatten)]
    pub(crate) grader: Grader,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "grader", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Grader {
    State {
        snapshot: PathBuf,
        schema: PathBuf,
    },
    Rubric {
        sessions: Option<Vec<String>>,
        #[serde(default)]
        evidence: Vec<String>,
        reference: String,
    },
    Command {
        run: PathBuf,
    },
}

#[derive(Clone)]
pub(crate) struct Task {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) hash: String,
    pub(crate) definition: Definition,
    pub(crate) mocks: BTreeMap<String, Mock>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mock {
    pub(crate) host: String,
    #[serde(default)]
    pub(crate) route: Vec<Route>,
    #[serde(default)]
    pub(crate) collection: Vec<Collection>,
    pub(crate) auth: Option<Auth>,
}

/// A header every request must carry: `prefix` followed by the value of the task's `variable`.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Auth {
    pub(crate) header: String,
    #[serde(default)]
    pub(crate) prefix: String,
    pub(crate) variable: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Route {
    pub(crate) method: String,
    pub(crate) path: String,
    #[serde(default = "ok_status", deserialize_with = "read_status")]
    pub(crate) status: hyper::StatusCode,
    #[serde(deserialize_with = "read_header")]
    pub(crate) content_type: hyper::header::HeaderValue,
    pub(crate) body: PathBuf,
}

fn ok_status() -> hyper::StatusCode {
    hyper::StatusCode::OK
}

fn read_status<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<hyper::StatusCode, D::Error> {
    hyper::StatusCode::from_u16(u16::deserialize(reader)?).map_err(serde::de::Error::custom)
}

fn read_header<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<hyper::header::HeaderValue, D::Error> {
    hyper::header::HeaderValue::from_str(&String::deserialize(reader)?)
        .map_err(serde::de::Error::custom)
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Collection {
    pub(crate) name: String,
    /// Where the collection is served; `/<name>` when absent.
    path: Option<String>,
    pub(crate) data: Option<PathBuf>,
    /// Fields the service fills in when a created item leaves them out.
    #[serde(default)]
    pub(crate) defaults: serde_json::Map<String, Value>,
    /// The query parameters a listing matches items by; the service ignores any other.
    #[serde(default)]
    pub(crate) filters: Vec<String>,
}

impl Collection {
    pub(crate) fn path(&self) -> String {
        self.path
            .clone()
            .unwrap_or_else(|| format!("/{}", self.name))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expectation {
    pub(crate) start: Option<toml::value::Datetime>,
    pub(crate) success: Option<bool>,
    #[serde(default)]
    pub(crate) criteria: BTreeMap<String, crate::grade::Kind>,
}

impl Task {
    pub(crate) async fn read(root: PathBuf) -> Result<Self> {
        let root = tokio::fs::canonicalize(root)
            .await
            .context("task directory")?;
        let id = root
            .file_name()
            .and_then(|s| s.to_str())
            .context("task directory name")?
            .to_owned();
        let definition: Definition = read_toml(&root.join("task.toml")).await?;
        ensure!(
            !definition.input.is_empty() && !definition.criteria.is_empty(),
            "{id}: input and criteria must not be empty"
        );
        ensure!(
            matches!(definition.category.as_str(), "assistant" | "programming"),
            "{id}: unknown category"
        );
        ensure!(
            !definition.mechanism.trim().is_empty(),
            "{id}: mechanism is empty"
        );
        ensure!(
            definition.budget.max_rounds > 0 && definition.budget.timeout_secs > 0,
            "{id}: budget must be positive"
        );
        let mut ids = BTreeSet::new();
        for criterion in &definition.criteria {
            ensure!(
                ids.insert(&criterion.id),
                "{id}: duplicate criterion {}",
                criterion.id
            );
            safe_name(&criterion.id)?;
        }
        for input in &definition.input {
            safe_name(&input.session)?;
        }
        let mut mocks = BTreeMap::new();
        for path in files(&root).await? {
            if path.parent() == Some(root.join("hidden/mocks").as_path())
                && path.extension().is_some_and(|s| s == "toml")
            {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("mock name")?
                    .to_owned();
                let mock: Mock = read_toml(&path).await?;
                ensure!(
                    !mocks.values().any(|m: &Mock| m.host == mock.host),
                    "{id}: duplicate mock host {}",
                    mock.host
                );
                mocks.insert(name, mock);
            }
        }
        let hash = hash(&root).await?;
        let task = Self {
            id,
            root,
            hash,
            definition,
            mocks,
        };
        validate(&task).await?;
        Ok(task)
    }

    pub(crate) fn mock_urls(&self) -> BTreeMap<String, String> {
        self.mocks
            .iter()
            .map(|(name, mock)| (name.clone(), format!("http://{}", mock.host)))
            .collect()
    }
}

pub(crate) fn safe_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']),
        "invalid name {name:?}"
    );
    Ok(())
}

pub(crate) async fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let source = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&source).with_context(|| format!("parse {}", path.display()))
}

pub(crate) async fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .with_context(|| format!("list {}", dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let kind = entry.file_type().await?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            } else {
                anyhow::bail!("unsupported fixture file type: {}", entry.path().display());
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) async fn hash(root: &Path) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    for path in files(root).await? {
        let name = path
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF-8 task path")?;
        let bytes = tokio::fs::read(&path).await?;
        hash.update(&(name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
        hash.update(
            &(tokio::fs::metadata(&path).await?.permissions().mode() & 0o111).to_le_bytes(),
        );
    }
    Ok(hash.finalize().to_hex().to_string())
}

async fn validate(task: &Task) -> Result<()> {
    let renderer = Renderer {
        start: chrono::Utc::now(),
        timezone: task.definition.environment.timezone,
        mocks: task.mock_urls(),
    };
    for criterion in &task.definition.criteria {
        match &criterion.grader {
            Grader::State { schema, .. } => {
                let source = tokio::fs::read_to_string(task.root.join(schema)).await?;
                let schema: Value = serde_json::from_str(&renderer.text(&source)?)?;
                jsonschema::options()
                    .with_draft(jsonschema::Draft::Draft202012)
                    .build(&schema)?;
            }
            Grader::Rubric { reference, .. } => {
                ensure!(
                    !reference.trim().is_empty(),
                    "{}: empty reference",
                    criterion.id
                );
                renderer.text(reference)?;
            }
            Grader::Command { run } => {
                ensure!(
                    task.root.join(run).is_file(),
                    "missing command grader {}",
                    run.display()
                );
            }
        }
    }
    for definition in task.mocks.values() {
        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for collection in &definition.collection {
            ensure!(
                names.insert(&collection.name),
                "{}: duplicate collection {}",
                definition.host,
                collection.name
            );
            let path = collection.path();
            ensure!(
                path.starts_with('/') && !path.ends_with('/') && paths.insert(path.clone()),
                "{}: collection path {path} must start with / and be unique",
                definition.host
            );
        }
        if let Some(auth) = &definition.auth {
            hyper::header::HeaderName::from_bytes(auth.header.as_bytes())?;
            ensure!(
                task.definition
                    .environment
                    .variables
                    .contains_key(&auth.variable),
                "{}: auth names variable {}, which [environment.variables] does not set",
                definition.host,
                auth.variable
            );
        }
        for route in &definition.route {
            let source = tokio::fs::read_to_string(task.root.join(&route.body)).await?;
            renderer.text(&source)?;
        }
        for collection in &definition.collection {
            if let Some(path) = &collection.data {
                let source = tokio::fs::read_to_string(task.root.join(path)).await?;
                let _: Vec<Value> = serde_json::from_str(&renderer.text(&source)?)?;
            }
        }
    }
    let home = task.root.join("home");
    if home.try_exists()? {
        for path in files(&home).await? {
            let bytes = tokio::fs::read(path).await?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                renderer.text(text)?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn select(repository: &Path, selected: &[String]) -> Result<Vec<PathBuf>> {
    let root = repository.join("evals/tasks");
    let mut names = selected.to_vec();
    if names.is_empty() {
        let mut entries = tokio::fs::read_dir(&root)
            .await
            .context("run eval-runner from the Enco repository root")?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                names.push(
                    entry
                        .file_name()
                        .to_str()
                        .context("task name is not UTF-8")?
                        .to_owned(),
                );
            }
        }
    }
    names.sort();
    names.dedup();
    ensure!(!names.is_empty(), "no evaluation tasks selected");
    let mut tasks = Vec::new();
    for name in names {
        safe_name(&name)?;
        tasks.push(root.join(name));
    }
    Ok(tasks)
}
