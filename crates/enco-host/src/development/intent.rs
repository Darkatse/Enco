use super::{BuildResult, Development, DevelopmentError, check_cancelled};
use enco_core::CodeIntent;
use enco_kernel::{ArtifactSource, RegisteredIntent, RegisteredSource, validate_plugin_name};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct Prepared {
    pub configuration: Option<(Vec<u8>, Value)>,
    pub plugins: BTreeMap<String, Change>,
    pub builds: Vec<BuildResult>,
}

/// File intent and the content offered to the registry are one prepared change.
pub(super) enum Change {
    Code {
        artifact: ArtifactSource,
        tree: String,
    },
    Factory,
    Artifact(Vec<u8>),
    Remove,
    Rename {
        to: String,
        tree: String,
    },
}

impl Prepared {
    pub fn changed(&self) -> bool {
        self.configuration.is_some() || !self.plugins.is_empty()
    }
}

impl Development {
    /// Compare against accepted intent once; the registry owns validation and settings changes.
    pub(super) async fn prepare(
        &self,
        baseline: &RegisteredIntent,
        cancel: &CancellationToken,
    ) -> Result<Prepared, DevelopmentError> {
        check_cancelled(cancel)?;
        self.git.check().await?;
        let directories = self.directories().await?;
        let registered = match &baseline.base {
            Some(record) => self.git.directories(&record.revision).await?,
            None => BTreeMap::new(),
        };
        let removed: Vec<_> = registered
            .keys()
            .filter(|name| !directories.contains(*name))
            .collect();
        if !removed.is_empty()
            && directories
                .iter()
                .any(|name| !registered.contains_key(name))
        {
            return Err(DevelopmentError::Invalid("Registered source is missing while a new directory appeared. Use plugin_rename for renaming; register deletion and creation separately.".into()));
        }
        self.sdk.write(&self.home).await?;
        let mut prepared = Prepared::default();
        let mut errors = Vec::new();
        match self.configuration().await {
            Ok((bytes, document))
                if baseline
                    .configuration
                    .as_ref()
                    .is_none_or(|config| config.document != document) =>
            {
                prepared.configuration = Some((bytes, document));
            }
            Ok(_) => {}
            Err(error) => errors.push(error.to_string()),
        }
        for name in directories {
            let code = baseline
                .plugins
                .get(&name)
                .and_then(|plugin| plugin.code.as_ref());
            let tree = registered.get(&name).map(String::as_str);
            match self.prepare_source(&name, tree, code, cancel).await {
                Ok(Some(built)) => {
                    if code.is_none_or(|code| {
                        code.kind != CodeIntent::Directory || code.artifact != built.artifact.hash()
                    }) {
                        prepared.plugins.insert(
                            name,
                            Change::Code {
                                artifact: built.artifact,
                                tree: built.tree,
                            },
                        );
                    }
                    prepared.builds.push(built.result);
                }
                Ok(None) => {}
                Err(DevelopmentError::Cancelled) => return Err(DevelopmentError::Cancelled),
                Err(error) => errors.push(error.to_string()),
            }
        }
        for name in removed {
            let factory = baseline
                .plugins
                .get(name)
                .is_some_and(|plugin| plugin.factory.is_some());
            prepared.plugins.insert(
                name.clone(),
                if factory {
                    Change::Factory
                } else {
                    Change::Remove
                },
            );
        }
        for (name, plugin) in &baseline.plugins {
            if plugin
                .factory
                .as_ref()
                .is_some_and(|factory| factory.follow)
            {
                prepared
                    .plugins
                    .entry(name.clone())
                    .or_insert(Change::Factory);
            }
        }
        if !errors.is_empty() {
            return Err(DevelopmentError::Build(errors.join("\n\n")));
        }
        Ok(prepared)
    }

    async fn prepare_source(
        &self,
        name: &str,
        registered: Option<&str>,
        code: Option<&RegisteredSource>,
        cancel: &CancellationToken,
    ) -> Result<Option<super::build::Built>, DevelopmentError> {
        check_cancelled(cancel)?;
        let source = self.git.tree(&format!("plugins/{name}")).await?;
        let artifact = code
            .filter(|code| code.kind == CodeIntent::Directory)
            .map(|code| code.artifact);
        if registered == Some(source.as_str())
            && self
                .store
                .build(&source, self.sdk.hash())
                .await?
                .is_some_and(|build| Some(build.artifact) == artifact)
        {
            return Ok(None);
        }
        self.build(name, source, artifact, cancel).await.map(Some)
    }

    pub(super) async fn directories(&self) -> Result<BTreeSet<String>, DevelopmentError> {
        let path = self.home.join("plugins");
        let mut entries = match tokio::fs::read_dir(&path).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeSet::new());
            }
            Err(error) => return Err(error.into()),
        };
        let mut names = BTreeSet::new();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let name = entry.file_name().into_string().map_err(|_| {
                DevelopmentError::Invalid(format!("{}: plugin name is not UTF-8", path.display()))
            })?;
            validate_plugin_name(&name)?;
            names.insert(name);
        }
        Ok(names)
    }

    pub(super) async fn configuration(&self) -> Result<(Vec<u8>, Value), DevelopmentError> {
        let path = self.home.join("config.toml");
        let input_error = |reason: String| DevelopmentError::Input {
            path: path.clone(),
            reason,
        };
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| input_error(error.to_string()))?;
        let text = std::str::from_utf8(&bytes).map_err(|error| input_error(error.to_string()))?;
        let document: toml::Value = toml::from_str(text)
            .map_err(|error: toml::de::Error| input_error(error.to_string()))?;
        Ok((bytes, serde_json::to_value(document)?))
    }
}
