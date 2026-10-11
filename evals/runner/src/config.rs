use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) model: PathBuf,
    pub(crate) judge: Judge,
    #[serde(default)]
    pub(crate) trial: Trial,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Judge {
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key_env: String,
    #[serde(default)]
    pub(crate) temperature: f64,
}

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Trial {
    pub(crate) root: PathBuf,
    pub(crate) user: String,
    pub(crate) path: String,
}

impl Default for Trial {
    fn default() -> Self {
        Self {
            root: std::env::temp_dir(),
            user: "lin".into(),
            path: "/usr/local/bin:/usr/bin:/bin".into(),
        }
    }
}

impl Config {
    pub(crate) async fn read(path: &Path, repository: &Path) -> Result<Self> {
        let source = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read {}", path.display()))?;
        let mut config: Self =
            toml::from_str(&source).with_context(|| format!("parse {}", path.display()))?;
        let parent = path
            .parent()
            .context("configuration has no parent directory")?;
        config.model = parent.join(&config.model);
        config.trial.root = std::path::absolute(parent.join(&config.trial.root))?;
        tokio::fs::create_dir_all(&config.trial.root).await?;
        config.trial.root = tokio::fs::canonicalize(&config.trial.root).await?;
        ensure!(
            !config.trial.root.starts_with(repository),
            "trial.root must be outside the evaluator repository"
        );
        ensure!(
            Path::new(&config.trial.user)
                .file_name()
                .and_then(|s| s.to_str())
                == Some(config.trial.user.as_str()),
            "trial.user must be one directory name"
        );
        reqwest::Url::parse(&config.judge.base_url).context("judge.base_url")?;
        ensure!(
            config.judge.temperature.is_finite(),
            "judge.temperature must be finite"
        );
        Ok(config)
    }
}

#[derive(Clone)]
pub(crate) struct Model {
    pub(crate) identity: ModelRecord,
    document: toml::Table,
    pub(crate) variables: BTreeMap<String, OsString>,
    pub(crate) hosts: BTreeSet<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ModelRecord {
    pub(crate) hash: String,
    pub(crate) endpoints: BTreeMap<String, EndpointRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct EndpointRecord {
    plugin: String,
    base_url: String,
    model: String,
}

impl Model {
    pub(crate) async fn read(path: &Path) -> Result<Self> {
        let source = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read model configuration {}", path.display()))?;
        Self::parse(source)
    }

    pub(crate) fn parse(source: String) -> Result<Self> {
        let document: toml::Table = toml::from_str(&source).context("parse model configuration")?;
        ensure!(
            document.keys().all(|key| matches!(
                key.as_str(),
                "endpoint" | "profile" | "embedding" | "decision"
            )),
            "model configuration may contain only endpoint, profile, embedding and decision"
        );
        for key in ["endpoint", "profile", "embedding"] {
            ensure!(
                document.contains_key(key),
                "model configuration is missing {key}"
            );
        }
        let mut variables = BTreeMap::new();
        let mut hosts = BTreeSet::new();
        let mut endpoints = BTreeMap::new();
        for (name, endpoint) in Self::endpoints(&document)? {
            let record: EndpointRecord = endpoint
                .clone()
                .try_into()
                .with_context(|| format!("model {name}"))?;
            let url = reqwest::Url::parse(&record.base_url)
                .with_context(|| format!("{name}.base_url"))?;
            hosts.insert(url.host_str().context("endpoint has no host")?.to_owned());
            if let Some(key) = endpoint.get("api_key_env").and_then(toml::Value::as_str) {
                let value = std::env::var_os(key)
                    .with_context(|| format!("model key variable {key} is not set"))?;
                variables.insert(key.to_owned(), value);
            }
            endpoints.insert(name, record);
        }
        Ok(Self {
            identity: ModelRecord {
                hash: blake3::hash(source.as_bytes()).to_hex().to_string(),
                endpoints,
            },
            document,
            variables,
            hosts,
        })
    }

    fn endpoints(document: &toml::Table) -> Result<BTreeMap<String, &toml::Value>> {
        let mut endpoints: BTreeMap<_, _> = document
            .get("endpoint")
            .and_then(toml::Value::as_table)
            .context("endpoint must be a table")?
            .iter()
            .map(|(name, value)| (format!("endpoint.{name}"), value))
            .collect();
        for name in ["embedding", "decision"] {
            if let Some(value) = document.get(name) {
                endpoints.insert(name.into(), value);
            }
        }
        Ok(endpoints)
    }

    pub(crate) fn configuration(&self, timezone: chrono_tz::Tz, max_rounds: u32) -> Result<String> {
        let mut document = self.document.clone();
        document.insert("timezone".into(), timezone.name().into());
        document.insert(
            "run".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "max_rounds".into(),
                toml::Value::Integer(i64::from(max_rounds)),
            )])),
        );
        Ok(toml::to_string(&document)?)
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Variant {
    pub(crate) enco: Option<PathBuf>,
    #[serde(default)]
    pub(crate) deploy: Vec<Deploy>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Deploy {
    pub(crate) plugin: String,
    pub(crate) artifact: PathBuf,
}

impl Variant {
    pub(crate) async fn read(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let source = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read variant {}", path.display()))?;
        let mut variant: Self = toml::from_str(&source).context("parse variant")?;
        let parent = path.parent().context("variant has no parent directory")?;
        variant.enco = variant.enco.map(|path| parent.join(path));
        for deploy in &mut variant.deploy {
            deploy.artifact = tokio::fs::canonicalize(parent.join(&deploy.artifact))
                .await
                .with_context(|| format!("artifact for {}", deploy.plugin))?;
        }
        Ok(variant)
    }
}
