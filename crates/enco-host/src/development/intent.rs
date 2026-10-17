use super::{
    BuildResult, Development, DevelopmentError, EntryError, EntryProblem, WorkspaceComparison,
    check_cancelled,
    error::{entry, io},
    tree::FileTree,
};
use enco_core::{CodeIntent, ContentHash, IntentPath, PLUGINS_DIRECTORY, PluginName};
use enco_kernel::{ArtifactSource, RegisteredIntent, RegisteredSource};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct Prepared {
    pub configuration: Option<ConfigurationInput>,
    pub plugins: BTreeMap<PluginName, Change>,
    pub builds: Vec<BuildResult>,
    pub text_only: Vec<IntentPath>,
}

/// Compare parsed intent while retaining the original bytes for history and rewind.
pub(super) struct ConfigurationInput {
    pub bytes: Vec<u8>,
    pub document: Value,
}

/// File intent and the content offered to the registry are one prepared change.
pub(super) enum Change {
    Code {
        artifact: ArtifactSource,
        tree: FileTree,
    },
    Factory,
    Artifact(Vec<u8>),
    Remove,
    Rename(PluginName),
}

impl Prepared {
    fn source(
        &mut self,
        name: PluginName,
        built: super::build::Built,
        code: Option<&RegisteredSource>,
    ) {
        let changed = code.is_none_or(|code| !code.matches_source_artifact(built.artifact.hash()));
        let comparison = WorkspaceComparison::between(
            Some(built.tree.hash()),
            code.and_then(RegisteredSource::source),
            Some(!changed),
        );
        if comparison == WorkspaceComparison::TextOnly {
            self.text_only.push(IntentPath::Plugin(name.clone()));
        }
        let (artifact, tree, result) = built.result(name.clone(), changed);
        if changed {
            self.plugins.insert(name, Change::Code { artifact, tree });
        }
        self.builds.push(result);
    }

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
        let directories = self.directories().await?;
        let registered = directory_sources(&baseline.plugins);
        let missing: Vec<_> = registered
            .keys()
            .filter(|name| !directories.contains(*name))
            .cloned()
            .collect();
        let added: Vec<_> = directories
            .iter()
            .filter(|name| !registered.contains_key(*name))
            .cloned()
            .collect();
        if !missing.is_empty() && !added.is_empty() {
            return Err(DevelopmentError::AmbiguousRename {
                missing,
                added,
                latest: baseline.accepted()?.id,
            });
        }
        self.sdk.write(&self.home).await?;
        let mut prepared = Prepared::default();
        let mut errors = Vec::new();
        match self.configuration().await {
            Ok(configuration) => match configuration.comparison(baseline.configuration()) {
                WorkspaceComparison::Changed => prepared.configuration = Some(configuration),
                WorkspaceComparison::TextOnly => prepared.text_only.push(IntentPath::Configuration),
                WorkspaceComparison::Same
                | WorkspaceComparison::Unbuilt
                | WorkspaceComparison::Removed
                | WorkspaceComparison::Absent
                | WorkspaceComparison::Unreadable(_) => {}
            },
            Err(error) => errors.push(error.into()),
        }
        for name in directories {
            let code = baseline
                .plugins
                .get(&name)
                .and_then(|plugin| plugin.code.as_ref());
            match self.prepare_source(&name, code, cancel).await {
                Ok(Some(built)) => prepared.source(name, built, code),
                Ok(None) => {}
                Err(DevelopmentError::Cancelled) => return Err(DevelopmentError::Cancelled),
                Err(error) => errors.push(error),
            }
        }
        for name in &missing {
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
            return Err(DevelopmentError::Preparation(errors));
        }
        Ok(prepared)
    }

    async fn prepare_source(
        &self,
        name: &PluginName,
        code: Option<&RegisteredSource>,
        cancel: &CancellationToken,
    ) -> Result<Option<super::build::Built>, DevelopmentError> {
        check_cancelled(cancel)?;
        let source = self.source_input(name, code).await?;
        let artifact = code
            .filter(|code| matches!(code.intent, CodeIntent::Directory { .. }))
            .map(|code| code.artifact);
        if matches!(source.comparison, WorkspaceComparison::Same) && !source.earlier_sdk {
            return Ok(None);
        }
        self.build(name, source.tree, source.cached, artifact, cancel)
            .await
            .map(Some)
    }

    /// Every directory under `plugins`; each must carry a plugin name.
    pub(super) async fn directories(&self) -> Result<BTreeSet<PluginName>, EntryError> {
        let path = self.home.join(PLUGINS_DIRECTORY);
        let mut entries = match tokio::fs::read_dir(&path).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeSet::new());
            }
            Err(error) => return Err(io(&path, &error)),
        };
        let mut names = BTreeSet::new();
        while let Some(item) = entries
            .next_entry()
            .await
            .map_err(|error| io(&path, &error))?
        {
            let location = item.path();
            let kind = item
                .file_type()
                .await
                .map_err(|error| io(&location, &error))?;
            if !kind.is_dir() && !kind.is_symlink() {
                continue;
            }
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| entry(&location, EntryProblem::NotUtf8))?;
            names.insert(
                name.parse()
                    .map_err(|_| entry(&location, EntryProblem::NotPluginName))?,
            );
        }
        Ok(names)
    }

    pub(super) async fn configuration(&self) -> Result<ConfigurationInput, EntryError> {
        let path = IntentPath::Configuration.locate(&self.home);
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| io(&path, &error))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| entry(&path, EntryProblem::NotText))?;
        let document: toml::Value = toml::from_str(text).map_err(|error: toml::de::Error| {
            entry(&path, EntryProblem::Toml(error.to_string()))
        })?;
        let document = serde_json::to_value(document)
            .map_err(|error| entry(&path, EntryProblem::Toml(error.to_string())))?;
        Ok(ConfigurationInput { bytes, document })
    }
}

pub(super) fn directory_sources(
    plugins: &BTreeMap<PluginName, enco_kernel::RegisteredPlugin>,
) -> BTreeMap<PluginName, ContentHash> {
    plugins
        .iter()
        .filter_map(|(name, plugin)| plugin.source().map(|source| (name.clone(), source)))
        .collect()
}
