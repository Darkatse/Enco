//! Recovery follows retained references and never reconstructs intent.
use super::*;
use crate::ActivationWrite;

pub(super) struct Recovery {
    /// The plugin recovering.
    pub plugin: PluginId,
    /// The reference recovery reaches, with what it loaded.
    pub target: Option<(PluginActivation, Loaded)>,
    pub failed: Vec<(GenerationId, Failure)>,
}

impl Recovery {
    /// The code recovery reaches and what it loaded, absent when nothing can run.
    pub fn loaded(self) -> Option<(CodeRef, Loaded)> {
        let (reference, loaded) = self.target?;
        Some((
            CodeRef {
                plugin: self.plugin,
                code: reference.code(),
            },
            loaded,
        ))
    }
}

pub(super) struct Candidate {
    reference: PluginActivation,
    artifact: ContentHash,
    settings: PluginSettings,
    failed: bool,
    loaded: Option<Loaded>,
}

/// The retained rollback target, then this host's factory code.
pub(super) fn candidates(
    state: &State,
    plugin: &PluginRecord,
) -> Result<Vec<Candidate>, RegistryError> {
    let factory = state
        .factory
        .get(&plugin.id)
        .map(|artifact| PluginActivation::Factory {
            artifact: *artifact,
            registration: None,
        });
    let mut seen = Vec::new();
    [plugin.rollback_target, factory]
        .into_iter()
        .flatten()
        .filter(|reference| {
            if Some(*reference) == plugin.active || seen.contains(reference) {
                return false;
            }
            seen.push(*reference);
            true
        })
        .map(|reference| {
            let code = CodeRef {
                plugin: plugin.id,
                code: reference.code(),
            };
            let (artifact, settings) = state.content(code)?;
            Ok(Candidate {
                failed: matches!(state.health(code), Some(Health::Failed { .. })),
                loaded: state.loaded.get(&code).cloned(),
                artifact,
                settings,
                reference,
            })
        })
        .collect()
}

/// Try the candidates of `plugin` in order; the first that loads is what recovery reaches.
pub(super) async fn prepare(
    deps: &RegistryDeps,
    plugin: PluginId,
    candidates: Vec<Candidate>,
) -> Result<Recovery, RegistryError> {
    let mut recovery = Recovery {
        plugin,
        target: None,
        failed: Vec::new(),
    };
    for candidate in candidates {
        if candidate.failed {
            continue;
        }
        let loaded = compile(
            deps,
            &candidate.artifact,
            &candidate.settings,
            candidate.loaded,
        )
        .await?;
        match loaded {
            Ok(loaded) => {
                recovery.target = Some((candidate.reference, loaded));
                break;
            }
            // Factory code is never on trial, so only a generation records the failure.
            Err(failure) => {
                if let Some(id) = candidate.reference.code().generation(plugin) {
                    recovery.failed.push((id, failure));
                }
            }
        }
    }
    Ok(recovery)
}

/// Every recovery writes retained factory bytes before its new references.
pub(super) async fn commit(
    deps: &RegistryDeps,
    state: &mut State,
    change: &ActivationWrite,
) -> Result<(), RegistryError> {
    let result = async {
        factory::retain(deps, &change.plugins).await?;
        deps.store.activate(change).await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        return Err(cleanup_failed_write(deps, state, error).await);
    }
    state.activated(change);
    Ok(())
}

/// Storage failures propagate; an absent or incompatible artifact is an attributable load failure.
pub(super) async fn load(
    deps: &RegistryDeps,
    state: &State,
    code: CodeRef,
    cached: Option<Loaded>,
) -> Result<Result<Loaded, Failure>, RegistryError> {
    let (artifact, settings) = state.content(code)?;
    compile(deps, &artifact, &settings, cached).await
}

/// The artifact must still be readable even when a compiled copy is cached.
async fn compile(
    deps: &RegistryDeps,
    artifact: &ContentHash,
    settings: &PluginSettings,
    cached: Option<Loaded>,
) -> Result<Result<Loaded, Failure>, RegistryError> {
    let bytes = match super::registration::artifact(deps, artifact).await {
        Ok(bytes) => bytes,
        Err(RegistryError::Store(StoreError::Artifact(hash))) => {
            return Ok(Err(load_failure(format!(
                "artifact {hash} is corrupt or missing"
            ))));
        }
        Err(error) => return Err(error),
    };
    if let Some(loaded) = cached {
        return Ok(Ok(loaded));
    }
    Ok(deps
        .runtime
        .load(&bytes, settings)
        .await
        .map_err(|error| load_failure(error.to_string())))
}

fn load_failure(message: String) -> Failure {
    Failure {
        code: code::PLUGIN_LOAD.into(),
        message,
        retryable: false,
    }
}

pub(super) async fn restore(deps: &RegistryDeps, state: &mut State) -> Result<(), RegistryError> {
    for plugin in state.node.plugins.values().cloned().collect::<Vec<_>>() {
        let Some(active) = state.active(plugin.id) else {
            continue;
        };
        if state.loaded.contains_key(&active) {
            continue;
        }
        match load(deps, state, active, None).await? {
            Ok(loaded) => {
                state.loaded.insert(active, loaded);
            }
            Err(failure) => {
                let mut recovery = prepare(deps, plugin.id, candidates(state, &plugin)?).await?;
                if let Some(id) = active.generation() {
                    recovery.failed.push((id, failure));
                }
                let mut restored = plugin;
                restored.active = recovery.target.as_ref().map(|(reference, _)| *reference);
                restored.rollback_target = None;
                let change = ActivationWrite {
                    plugins: vec![restored],
                    failed: std::mem::take(&mut recovery.failed),
                    ..ActivationWrite::default()
                };
                commit(deps, state, &change).await?;
                state.loaded.extend(recovery.loaded());
            }
        }
    }
    Ok(())
}

impl Registry {
    /// Restore the still-effective parts of the newest accepted change.
    pub async fn rollback(&self) -> Result<Rollback, RegistryError> {
        let RollbackPlan {
            expected,
            registration,
            mut change,
            recoveries,
        } = {
            let state = self.state.lock().await;
            rollback_plan(&state)?
        };
        let mut loaded = BTreeMap::new();
        for (id, candidates) in recoveries {
            let mut recovery = prepare(&self.deps, id, candidates).await?;
            if let Some(plugin) = change.plugins.iter_mut().find(|plugin| plugin.id == id) {
                plugin.active = recovery.target.as_ref().map(|(reference, _)| *reference);
                plugin.rollback_target = None;
            }
            change.failed.append(&mut recovery.failed);
            loaded.extend(recovery.loaded());
        }
        let mut state = self.state.lock().await;
        if !expected.matches(&state) {
            return Err(RegistryError::Conflict("rollback".into()));
        }
        let config = change
            .configuration
            .map(|id| {
                let record = state.intent.configurations.get(&id).ok_or_else(|| {
                    RegistryError::Intent(format!("registration {id} has no configuration"))
                })?;
                configuration::declare(&record.document, &self.deps)
                    .map(|declaration| declaration.into_active(record))
                    .map_err(RegistryError::Config)
            })
            .transpose()?;
        commit(&self.deps, &mut state, &change).await?;
        state.loaded.extend(loaded);
        self.publish(&mut state, config, None)?;
        let warnings = cleanup_artifacts(&self.deps, &state)
            .await
            .into_iter()
            .collect();
        Ok(Rollback {
            registration,
            plugins: change.plugins,
            configuration: change.configuration,
            warnings,
        })
    }
}

struct RollbackPlan {
    expected: registration::Expected,
    registration: RegistrationId,
    change: ActivationWrite,
    recoveries: BTreeMap<PluginId, Vec<Candidate>>,
}

/// What this node ran before the newest still-effective registration, from its own record.
fn rollback_plan(state: &State) -> Result<RollbackPlan, RegistryError> {
    let initial = state.intent.registrations.keys().next().copied();
    let registration = state
        .intent
        .registrations
        .values()
        .rev()
        .find(|record| Some(record.id) != initial && effective(state, record))
        .ok_or(RegistryError::NoRollbackTarget)?;
    let replaced = state.node.replaced.get(&registration.id).ok_or_else(|| {
        RegistryError::Intent(format!(
            "this node has no record of what registration {} replaced",
            registration.id
        ))
    })?;
    let mut change = ActivationWrite::default();
    if state.node.configuration == Some(registration.id) {
        change.configuration = replaced.configuration;
    }
    let mut recoveries = BTreeMap::new();
    for plugin in state.node.plugins.values() {
        let code = plugin
            .active
            .as_ref()
            .is_some_and(|active| active.registration() == Some(registration.id));
        let name = plugin.named_by == Some(registration.id);
        if !code && !name {
            continue;
        }
        let mut restored = plugin.clone();
        if name {
            restored.named_by = replaced.names.get(&plugin.id).copied().flatten();
        }
        let added = name && restored.named_by.is_none();
        let removed = name
            && registration
                .names
                .get(&plugin.id)
                .is_some_and(|row| row.name.is_none());
        if added {
            restored.active = None;
            restored.rollback_target = None;
        } else if code || removed {
            recoveries.insert(plugin.id, candidates(state, plugin)?);
        }
        change.plugins.push(restored);
    }
    Ok(RollbackPlan {
        expected: registration::Expected::capture(state),
        registration: registration.id,
        change,
        recoveries,
    })
}

fn effective(state: &State, record: &RegistrationRecord) -> bool {
    state.node.configuration == Some(record.id)
        || state.node.plugins.values().any(|plugin| {
            plugin.named_by == Some(record.id)
                || plugin
                    .active
                    .as_ref()
                    .is_some_and(|active| active.registration() == Some(record.id))
        })
}
