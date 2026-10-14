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
            document
                .keys()
                .all(|key| matches!(key.as_str(), "endpoint" | "profile" | "memory")),
            "model configuration may contain only endpoint, profile and memory"
        );
        for key in ["endpoint", "profile", "memory"] {
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
                pass(&mut variables, key).with_context(|| format!("{name}.api_key_env"))?;
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
        Ok(document
            .get("endpoint")
            .and_then(toml::Value::as_table)
            .context("endpoint must be a table")?
            .iter()
            .map(|(name, value)| (format!("endpoint.{name}"), value))
            .collect())
    }

    pub(crate) fn configuration(
        &self,
        timezone: chrono_tz::Tz,
        max_rounds: u32,
        plugins: &toml::Table,
    ) -> Result<String> {
        let mut document = self.document.clone();
        document.insert(
            "owner".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "timezone".into(),
                timezone.name().into(),
            )])),
        );
        document.insert(
            "run".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "max_rounds".into(),
                toml::Value::Integer(i64::from(max_rounds)),
            )])),
        );
        if !plugins.is_empty() {
            document.insert("plugin".into(), toml::Value::Table(plugins.clone()));
        }
        Ok(toml::to_string(&document)?)
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Variant {
    pub(crate) enco: Option<PathBuf>,
    #[serde(default)]
    pub(crate) deploy: Vec<Deploy>,
    #[serde(default = "default_plugins")]
    pub(crate) plugin: toml::Table,
    /// The variables named by `plugin.*.secrets`.
    #[serde(skip)]
    pub(crate) variables: BTreeMap<String, OsString>,
}

impl Default for Variant {
    fn default() -> Self {
        Self {
            enco: None,
            deploy: Vec::new(),
            plugin: default_plugins(),
            variables: BTreeMap::new(),
        }
    }
}

/// The owner's web setup, kept inside the trial's network: Jina is the runner's reader, and the
/// search services are hosts a task may mock; the proxy refuses them like any other unknown host.
pub(crate) fn default_plugins() -> toml::Table {
    fn table<const N: usize>(entries: [(&str, toml::Value); N]) -> toml::Value {
        toml::Value::Table(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }
    let urls = table([
        ("jina", crate::reader::URL.into()),
        ("exa", "http://api.exa.ai".into()),
        ("tinyfish", "http://api.search.tinyfish.ai".into()),
    ]);
    let search = toml::Value::Array(vec!["exa".into(), "tinyfish".into()]);
    let secrets = table([
        ("exa", "EXA_API_KEY".into()),
        ("tinyfish", "TINYFISH_API_KEY".into()),
    ]);
    let web = table([
        ("config", table([("search", search), ("urls", urls)])),
        ("secrets", secrets),
    ]);
    toml::Table::from_iter([("web".to_owned(), web)])
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Deploy {
    pub(crate) plugin: String,
    pub(crate) artifact: PathBuf,
}

impl Variant {
    /// The variant in a file, or the default one; either way its plugin secrets must be set.
    pub(crate) async fn read(path: Option<&Path>) -> Result<Self> {
        let mut variant = match path {
            Some(path) => Self::parse(path).await?,
            None => Self::default(),
        };
        let plugins: BTreeMap<String, PluginSecrets> = toml::Value::Table(variant.plugin.clone())
            .try_into()
            .context("variant plugin")?;
        for (plugin, settings) in plugins {
            for (secret, name) in settings.secrets {
                pass(&mut variant.variables, &name)
                    .with_context(|| format!("plugin.{plugin}.secrets.{secret}"))?;
            }
        }
        Ok(variant)
    }

    async fn parse(path: &Path) -> Result<Self> {
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

/// The part of a plugin's settings the runner reads; the daemon validates the rest.
#[derive(Deserialize)]
struct PluginSecrets {
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

/// Configuration names environment variables instead of holding secrets: a named variable must be
/// set for the runner, and it is passed to the daemon, whose environment is otherwise the trial's.
fn pass(variables: &mut BTreeMap<String, OsString>, name: &str) -> Result<()> {
    let value = std::env::var_os(name)
        .with_context(|| format!("environment variable {name} is not set"))?;
    variables.insert(name.to_owned(), value);
    Ok(())
}
