//! Writing an earlier registration's files back into the home, without registering them.
//!
//! A rewind prepares every file operation before it changes any file in the home.
use super::{
    ContentUnavailable, Development, DevelopmentError, Failed, HistoryPoint, Operation,
    RewindArguments, check_cancelled,
    error::io,
    tree::{Directory, Effect, FileTree, Ownership, SourcePath, WriteConflict, WritePlan},
};
use enco_core::{ConfigGeneration, ContentHash, IntentPath, PluginName, RegistrationId};
use enco_kernel::{Registry, ScopeError, Store};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};
use tokio_util::sync::CancellationToken;

/// What a rewind wrote back into the home.
#[derive(Serialize)]
pub struct Rewound {
    /// The registration whose files were written back.
    pub registration: RegistrationId,
    /// The paths whose files were written.
    pub written: Vec<IntentPath>,
    /// The plugin directories removed, because the registration had no plugin there.
    pub removed: Vec<IntentPath>,
    /// The selected paths that already matched the registration.
    pub unchanged: Vec<IntentPath>,
}

/// What keeps a rewind from writing one path.
#[derive(Debug, Serialize)]
pub struct RewindConflict {
    /// The path the rewind would write.
    pub path: IntentPath,
    /// What stands in the way there.
    pub conflicts: Vec<WriteConflict>,
}

struct UnitWrite {
    path: IntentPath,
    plan: WritePlan,
}

struct PluginTarget {
    name: PluginName,
    /// The source registered by the target registration; absent removes the directory.
    source: Option<ContentHash>,
    /// The source registered by the latest registration, which protects unregistered edits.
    registered: Option<ContentHash>,
}

impl Development {
    /// Write the files of `arguments.registration` back to the selected paths; nothing is
    /// registered.
    pub async fn rewind(
        &self,
        registry: &Registry,
        arguments: RewindArguments,
        cancel: &CancellationToken,
    ) -> Result<Rewound, Failed> {
        let _operation = self.operation.lock().await;
        let result = match self.prepare_rewind(registry, &arguments, cancel).await {
            Ok(writes) => apply_rewind(arguments.registration, writes, cancel).await,
            Err(error) => Err(error),
        };
        result.map_err(|error| Operation::Rewind(arguments).failed(error))
    }

    async fn prepare_rewind(
        &self,
        registry: &Registry,
        arguments: &RewindArguments,
        cancel: &CancellationToken,
    ) -> Result<Vec<UnitWrite>, DevelopmentError> {
        let registration = arguments.registration;
        let target = registry.registered_intent(Some(registration)).await?;
        let latest = registry.registered_intent(None).await?;
        let scope = latest.rewind_scope(&target);
        let selection = match &arguments.paths {
            Some(paths) => scope.select(paths),
            None => scope.changed(),
        }
        .map_err(|error| match error {
            ScopeError::Unknown(name) => DevelopmentError::RewindPath {
                path: IntentPath::Plugin(name),
            },
            ScopeError::Collision(name) => DevelopmentError::RewindCollision {
                path: IntentPath::Plugin(name),
            },
        })?;
        let explicit = arguments.paths.is_some();
        let latest_id = latest.accepted()?.id;
        let selected: Vec<_> = selection
            .configuration
            .then_some(IntentPath::Configuration)
            .into_iter()
            .chain(selection.plugins.values().cloned().map(IntentPath::Plugin))
            .collect();
        let mut writes = Vec::new();
        let mut conflicts = Vec::new();
        if selection.configuration {
            let write = self
                .config_write(
                    &target.accepted()?.configuration,
                    &latest.accepted()?.configuration,
                    explicit,
                )
                .await?;
            match write {
                Ok(write) => writes.push(write),
                Err(blocked) => conflicts.push(blocked),
            }
        }
        for (id, name) in selection.plugins {
            check_cancelled(cancel)?;
            let plugin = PluginTarget {
                name,
                source: target
                    .plugin_by_id(id)
                    .and_then(|(_, plugin)| plugin.source()),
                registered: latest
                    .plugin_by_id(id)
                    .and_then(|(_, plugin)| plugin.source()),
            };
            match self
                .plugin_write(plugin, registration, latest_id, explicit)
                .await?
            {
                Ok(write) => writes.push(write),
                Err(blocked) => conflicts.push(blocked),
            }
        }
        if conflicts.is_empty() {
            Ok(writes)
        } else {
            Err(DevelopmentError::RewindRefused {
                conflicts,
                selected,
            })
        }
    }

    /// Plan one plugin directory; without explicit paths, files with unregistered changes block it.
    async fn plugin_write(
        &self,
        plugin: PluginTarget,
        registration: RegistrationId,
        latest: RegistrationId,
        explicit: bool,
    ) -> Result<Result<UnitWrite, RewindConflict>, DevelopmentError> {
        let path = IntentPath::Plugin(plugin.name);
        let root = path.locate(&self.home);
        let directory = Directory::read(&root, Ownership::Source).await?;
        let target = match plugin.source {
            Some(source) => Some(
                FileTree::load(self.store.as_ref(), source)
                    .await
                    .map_err(|error| content(&path, registration, error))?,
            ),
            None => None,
        };
        let mut blocked = Vec::new();
        if !explicit {
            let registered = match plugin.registered {
                Some(source) => FileTree::load_manifest(self.store.as_ref(), source)
                    .await
                    .map_err(|error| content(&path, latest, error))?,
                None => BTreeMap::new(),
            };
            blocked.extend(protect(
                &root,
                &directory.value,
                target.as_ref(),
                &registered,
            ));
        }
        let plan = match target {
            Some(target) => directory.plan_write(target),
            None => directory.plan_removal(),
        };
        Ok(match plan {
            Ok(plan) if blocked.is_empty() => Ok(UnitWrite { path, plan }),
            Ok(_) => Err(RewindConflict {
                path,
                conflicts: blocked,
            }),
            Err(conflicts) => {
                blocked.extend(conflicts);
                Err(RewindConflict {
                    path,
                    conflicts: blocked,
                })
            }
        })
    }

    async fn config_write(
        &self,
        target: &ConfigGeneration,
        baseline: &ConfigGeneration,
        explicit: bool,
    ) -> Result<Result<UnitWrite, RewindConflict>, DevelopmentError> {
        let location = IntentPath::Configuration.locate(&self.home);
        let current = match tokio::fs::read(&location).await {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io(&location, &error).into()),
        };
        let bytes = self.store.get_blob(&target.source).await.map_err(|error| {
            content(
                &IntentPath::Configuration,
                target.registration,
                error.into(),
            )
        })?;
        if !explicit
            && current.as_deref() != Some(bytes.as_slice())
            && current.as_deref().map(ContentHash::of) != Some(baseline.source)
        {
            return Ok(Err(RewindConflict {
                path: IntentPath::Configuration,
                conflicts: vec![WriteConflict::Unregistered { path: location }],
            }));
        }
        Ok(Ok(UnitWrite {
            path: IntentPath::Configuration,
            plan: WritePlan::file(location, current.as_deref(), bytes),
        }))
    }
}

fn content(
    path: &IntentPath,
    registration: RegistrationId,
    error: DevelopmentError,
) -> DevelopmentError {
    match ContentUnavailable::of(HistoryPoint::Registration(registration), error) {
        Ok(unavailable) => DevelopmentError::RewindContent {
            path: path.clone(),
            unavailable,
        },
        Err(error) => error,
    }
}

/// Files the rewind would change whose bytes differ from their registered value.
fn protect(
    root: &std::path::Path,
    current: &FileTree,
    target: Option<&FileTree>,
    registered: &BTreeMap<SourcePath, ContentHash>,
) -> Vec<WriteConflict> {
    let absent = FileTree::default();
    target
        .unwrap_or(&absent)
        .changes(current)
        .into_iter()
        .filter(|path| {
            current.files.get(*path).map(|bytes| ContentHash::of(bytes))
                != registered.get(*path).copied()
        })
        .map(|path| WriteConflict::Unregistered {
            path: root.join(path),
        })
        .collect()
}

async fn apply_rewind(
    registration: RegistrationId,
    writes: Vec<UnitWrite>,
    cancel: &CancellationToken,
) -> Result<Rewound, DevelopmentError> {
    let mut result = Rewound {
        registration,
        written: Vec::new(),
        removed: Vec::new(),
        unchanged: Vec::new(),
    };
    let mut completed: Vec<PathBuf> = Vec::new();
    for write in writes {
        let effect = write.plan.effect();
        let executed = match check_cancelled(cancel) {
            Ok(()) => write.plan.execute(&mut completed).await,
            Err(error) => Err(error),
        };
        if let Err(source) = executed {
            return Err(DevelopmentError::Partial {
                written: completed,
                source: Box::new(source),
            });
        }
        match effect {
            Effect::Unchanged => result.unchanged.push(write.path),
            Effect::Written => result.written.push(write.path),
            Effect::Removed => result.removed.push(write.path),
        }
    }
    Ok(result)
}
