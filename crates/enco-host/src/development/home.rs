//! How each part of the home compares with what is registered, decided without building.
use super::{
    Development, DevelopmentError, EntryError, cache::Build, intent::ConfigurationInput,
    tree::FileTree,
};
use enco_core::{ConfigGeneration, ContentHash, IntentPath, PluginName};
use enco_kernel::{RegisteredIntent, RegisteredSource};
use serde::Serialize;
use std::collections::BTreeMap;

/// How `config.toml` or a plugin directory in the home compares with what is registered.
///
/// Byte equality and meaning are distinct facts; reading a comparison never builds source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HomeComparison {
    /// The bytes are the registered ones.
    Same,
    /// The bytes differ but mean the same, as when only a comment changed; registering them
    /// registers nothing.
    TextOnly,
    /// The bytes differ, and so does what they mean.
    Changed,
    /// The bytes differ, and no build tells yet whether the code does.
    Unbuilt,
    /// The registered directory is gone from the home.
    Removed,
    /// There is no directory and no registered source.
    Absent,
    /// The directory or file cannot be read.
    Unreadable(EntryError),
}

impl HomeComparison {
    /// Known meaning is supplied by parsing configuration or by an existing build record.
    pub(super) fn between(
        current: Option<ContentHash>,
        registered: Option<ContentHash>,
        same_meaning: Option<bool>,
    ) -> Self {
        match (current, registered) {
            (None, None) => Self::Absent,
            (current, registered) if current == registered => Self::Same,
            (None, Some(_)) => Self::Removed,
            (Some(_), _) => match same_meaning {
                Some(true) => Self::TextOnly,
                Some(false) => Self::Changed,
                None => Self::Unbuilt,
            },
        }
    }
}

/// How a plugin's table in `config.toml` compares with the registered settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsComparison {
    /// The table is the registered one.
    Same,
    /// The table differs from the registered one.
    Changed,
    /// `config.toml` or its plugin tables cannot be read; the reason is reported for `config.toml`.
    Unknown,
}

impl ConfigurationInput {
    pub(super) fn comparison(&self, registered: Option<&ConfigGeneration>) -> HomeComparison {
        HomeComparison::between(
            Some(ContentHash::of(&self.bytes)),
            registered.map(|config| config.source),
            Some(registered.is_some_and(|config| config.document == self.document)),
        )
    }
}

/// How a plugin in the home compares with what is registered.
#[derive(Serialize)]
pub struct HomePlugin {
    /// Its source directory against the registered source.
    pub code: HomeComparison,
    /// Its table in `config.toml` against the registered settings.
    pub settings: SettingsComparison,
}

pub(super) struct HomeStatus {
    pub configuration: HomeComparison,
    pub plugins: Result<BTreeMap<PluginName, HomePlugin>, EntryError>,
}

pub(super) struct SourceInput {
    pub tree: FileTree,
    pub comparison: HomeComparison,
    pub cached: Option<Build>,
    pub earlier_sdk: bool,
}

impl Development {
    /// Compare the home with registered intent. Unreadable entries are facts of the
    /// comparison; any other failure stops it.
    pub(super) async fn compare(
        &self,
        baseline: &RegisteredIntent,
    ) -> Result<HomeStatus, DevelopmentError> {
        let config = self.configuration().await;
        let configuration = match &config {
            Ok(config) => config.comparison(baseline.configuration()),
            Err(error) => HomeComparison::Unreadable(error.clone()),
        };
        // A plugin table this host cannot read is reported as an invalid table of `config.toml`,
        // so here it only makes the settings comparison unknown.
        let tables = |document| enco_kernel::plugin_settings(document).ok();
        let registered_settings = match baseline.configuration() {
            Some(config) => tables(&config.document),
            None => Some(BTreeMap::new()),
        };
        let current_settings = config
            .as_ref()
            .ok()
            .and_then(|config| tables(&config.document));
        let directories = match self.directories().await {
            Ok(directories) => directories,
            Err(error) => {
                return Ok(HomeStatus {
                    configuration,
                    plugins: Err(error),
                });
            }
        };
        let mut plugins = BTreeMap::new();
        for name in baseline.plugins.keys().chain(&directories) {
            if plugins.contains_key(name) {
                continue;
            }
            let registered = baseline
                .plugins
                .get(name)
                .and_then(|plugin| plugin.code.as_ref());
            let code = if directories.contains(name) {
                match self.source_input(name, registered).await {
                    Ok(source) => source.comparison,
                    Err(DevelopmentError::Entry(error)) => HomeComparison::Unreadable(error),
                    Err(error) => return Err(error),
                }
            } else {
                HomeComparison::between(None, registered.and_then(RegisteredSource::source), None)
            };
            let settings = match (&current_settings, &registered_settings) {
                (Some(current), Some(registered)) if current.get(name) == registered.get(name) => {
                    SettingsComparison::Same
                }
                (Some(_), Some(_)) => SettingsComparison::Changed,
                _ => SettingsComparison::Unknown,
            };
            plugins.insert(name.clone(), HomePlugin { code, settings });
        }
        Ok(HomeStatus {
            configuration,
            plugins: Ok(plugins),
        })
    }

    pub(super) async fn source_input(
        &self,
        name: &PluginName,
        registered: Option<&RegisteredSource>,
    ) -> Result<SourceInput, DevelopmentError> {
        let tree = FileTree::read(&IntentPath::Plugin(name.clone()).locate(&self.home)).await?;
        let source = tree.hash();
        let cached = self.store.build(source, self.sdk.hash()).await?;
        let earlier_sdk = match registered {
            Some(code) => self.built_with_earlier_sdk(code).await?,
            None => false,
        };
        let same_meaning = cached.as_ref().map(|build| {
            registered.is_some_and(|code| code.matches_source_artifact(build.artifact))
        });
        let comparison = HomeComparison::between(
            Some(source),
            registered.and_then(RegisteredSource::source),
            same_meaning,
        );
        Ok(SourceInput {
            tree,
            comparison,
            cached,
            earlier_sdk,
        })
    }

    /// Whether a registered source has no build for this host's SDK that produced its code;
    /// the next registration then rebuilds it.
    pub(super) async fn built_with_earlier_sdk(
        &self,
        code: &RegisteredSource,
    ) -> Result<bool, DevelopmentError> {
        let Some(source) = code.source() else {
            return Ok(false);
        };
        let build = self.store.build(source, self.sdk.hash()).await?;
        Ok(build.is_none_or(|build| !code.matches_source_artifact(build.artifact)))
    }
}
