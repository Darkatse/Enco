use super::{Development, DevelopmentError};
use enco_core::{CodeIntent, PluginSettings};
use enco_kernel::{RegisteredIntent, RegisteredSource, Registry, RegistryError, RegistryStatus};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

/// A comparison reports only what can be established without building or changing intent.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RegisteredComparison {
    Unchanged,
    Changed,
    Removed,
    Unbuilt,
    Unknown { reason: String },
}

/// The independent code and configuration-section comparisons for a source name.
#[derive(Serialize)]
pub struct PluginWorkspace {
    pub path: PathBuf,
    pub code: RegisteredComparison,
    pub settings: RegisteredComparison,
    /// The registered source has not been built with this host's SDK.
    pub sdk_changed: Option<bool>,
}

/// Working files compared with the last accepted intent, never with running generations.
#[derive(Serialize)]
pub struct WorkspaceStatus {
    pub configuration: RegisteredComparison,
    pub plugins: BTreeMap<String, PluginWorkspace>,
    /// An error preventing a complete listing; known configuration facts remain available.
    pub unavailable: Option<String>,
    /// Authoritative accepted commit, even when the Git anchor could not be updated.
    pub registered: Option<String>,
    /// Current Git anchor; unavailable comparisons carry a reason above.
    pub reference: Option<String>,
    /// Host follow failures remain visible until those plugins accept new code intent.
    pub follow_rejections: BTreeMap<String, String>,
}

impl Development {
    /// Inspect intent without invoking Cargo. Git failures affect source comparisons only.
    pub async fn status(
        &self,
        registry: &Registry,
    ) -> Result<(RegistryStatus, WorkspaceStatus), RegistryError> {
        let baseline = registry.status().await?;
        let mut workspace = self.compare(&baseline.registered).await;
        let follow_rejections = self.follow_rejections.read().await;
        workspace.follow_rejections = baseline
            .plugins
            .iter()
            .filter_map(|plugin| {
                let reason = follow_rejections.get(&plugin.id)?;
                Some((plugin.factory.as_ref()?.name.clone(), reason.clone()))
            })
            .collect();
        Ok((baseline, workspace))
    }

    async fn compare(&self, baseline: &RegisteredIntent) -> WorkspaceStatus {
        let config = self.configuration().await.map(|(_, document)| document);
        let configuration = match &config {
            Ok(document) => compare(
                &Some(document),
                &baseline
                    .configuration
                    .as_ref()
                    .map(|config| &config.document),
            ),
            Err(error) => unknown(error),
        };
        let revision = baseline.base.as_ref().map(|record| record.revision.clone());
        let git = async {
            self.git.check().await?;
            let directories = match &revision {
                Some(revision) => self.git.directories(revision).await?,
                None => BTreeMap::new(),
            };
            Ok::<_, DevelopmentError>((directories, self.git.reference().await?))
        }
        .await;
        let known = &baseline.plugins;
        let directories = self.directories().await;
        let unavailable = directories
            .as_ref()
            .err()
            .or(git.as_ref().err())
            .map(ToString::to_string);
        let mut names: std::collections::BTreeSet<_> = known.keys().cloned().collect();
        if let Ok(directories) = &directories {
            names.extend(directories.iter().cloned());
        }
        let mut plugins = BTreeMap::new();
        for name in names {
            let code = known.get(&name).and_then(|plugin| plugin.code.as_ref());
            let (code, sdk_changed) = match (&directories, &git) {
                (Ok(directories), Ok((registered, _))) => {
                    self.source_status(
                        &name,
                        code,
                        directories.contains(&name),
                        registered.get(&name).map(String::as_str),
                    )
                    .await
                }
                (Err(error), _) | (_, Err(error)) => (unknown(error), None),
            };
            let settings = match &config {
                Ok(document) => match (
                    section(document, &name),
                    baseline
                        .configuration
                        .as_ref()
                        .map_or(Ok(None), |config| section(&config.document, &name)),
                ) {
                    (Ok(current), Ok(previous)) => compare(&current, &previous),
                    (Err(error), _) | (_, Err(error)) => unknown(error),
                },
                Err(error) => unknown(error),
            };
            plugins.insert(
                name.clone(),
                PluginWorkspace {
                    path: self.home.join("plugins").join(name),
                    code,
                    settings,
                    sdk_changed,
                },
            );
        }
        WorkspaceStatus {
            configuration,
            plugins,
            unavailable,
            registered: revision,
            reference: git.ok().and_then(|(_, reference)| reference),
            follow_rejections: BTreeMap::new(),
        }
    }

    async fn source_status(
        &self,
        name: &str,
        code: Option<&RegisteredSource>,
        present: bool,
        registered: Option<&str>,
    ) -> (RegisteredComparison, Option<bool>) {
        let result = async {
            let sdk_changed = match registered {
                Some(tree) => self
                    .store
                    .build(tree, self.sdk.hash())
                    .await?
                    .is_none_or(|build| code.is_none_or(|code| code.artifact != build.artifact)),
                None => false,
            };
            if !present {
                return Ok((
                    if registered.is_some() {
                        RegisteredComparison::Removed
                    } else {
                        RegisteredComparison::Unchanged
                    },
                    Some(sdk_changed),
                ));
            }
            let source = self.git.tree(&format!("plugins/{name}")).await?;
            if registered == Some(source.as_str()) {
                return Ok((RegisteredComparison::Unchanged, Some(sdk_changed)));
            }
            let build = self.store.build(&source, self.sdk.hash()).await?;
            let code = match build {
                Some(build)
                    if code.is_some_and(|code| {
                        code.kind == CodeIntent::Directory && code.artifact == build.artifact
                    }) =>
                {
                    RegisteredComparison::Unchanged
                }
                Some(_) => RegisteredComparison::Changed,
                None => RegisteredComparison::Unbuilt,
            };
            Ok::<_, DevelopmentError>((code, Some(sdk_changed)))
        }
        .await;
        match result {
            Ok(comparison) => comparison,
            Err(error) => (unknown(error), None),
        }
    }
}

fn section(document: &Value, name: &str) -> Result<Option<PluginSettings>, serde_json::Error> {
    document
        .get("plugin")
        .and_then(|plugins| plugins.get(name))
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
}

fn compare<T: PartialEq>(current: &T, registered: &T) -> RegisteredComparison {
    if current == registered {
        RegisteredComparison::Unchanged
    } else {
        RegisteredComparison::Changed
    }
}

fn unknown(reason: impl ToString) -> RegisteredComparison {
    RegisteredComparison::Unknown {
        reason: reason.to_string(),
    }
}
