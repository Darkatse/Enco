//! Recovery follows retained references and never reconstructs intent.
use super::*;
use crate::ActivationWrite;

pub(super) struct Recovery {
    pub target: Option<(PluginActivation, Loaded)>,
    pub failed: Vec<(GenerationId, Failure)>,
}

pub(super) struct Candidate {
    reference: PluginActivation,
    record: GenerationRecord,
    loaded: Option<Loaded>,
}

pub(super) fn candidates(state: &State, plugin: &PluginRecord) -> Vec<Candidate> {
    let factory = state
        .factory
        .get(&plugin.id)
        .map(|generation| PluginActivation {
            generation: *generation,
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
        .filter_map(|reference| {
            state
                .history
                .generations
                .get(&reference.generation)
                .map(|record| Candidate {
                    reference,
                    record: record.clone(),
                    loaded: state.loaded.get(&reference.generation).cloned(),
                })
        })
        .collect()
}

pub(super) async fn prepare(
    deps: &RegistryDeps,
    candidates: Vec<Candidate>,
) -> Result<Recovery, RegistryError> {
    let mut recovery = Recovery {
        target: None,
        failed: Vec::new(),
    };
    for candidate in candidates {
        if candidate.record.status == GenerationStatus::Failed {
            continue;
        }
        let loaded = load(deps, &candidate.record, candidate.loaded).await?;
        match loaded {
            Ok(loaded) => {
                recovery.target = Some((candidate.reference, loaded));
                break;
            }
            Err(failure) => recovery.failed.push((candidate.record.id, failure)),
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
        factory::retain(deps, state, &change.plugins).await?;
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
    record: &GenerationRecord,
    cached: Option<Loaded>,
) -> Result<Result<Loaded, Failure>, RegistryError> {
    let bytes = match super::registration::artifact(deps, record).await {
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
        .load(&bytes, &record.settings)
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
    for plugin in state.history.plugins.values().cloned().collect::<Vec<_>>() {
        let Some(active) = plugin.active else {
            continue;
        };
        let active = active.generation;
        if state.loaded.contains_key(&active) {
            continue;
        }
        let record = state
            .history
            .generations
            .get(&active)
            .ok_or(RegistryError::UnknownGeneration(active))?;
        match load(deps, record, None).await? {
            Ok(loaded) => {
                state.loaded.insert(active, loaded);
            }
            Err(failure) => {
                let mut recovery = prepare(deps, candidates(state, &plugin)).await?;
                recovery.failed.push((active, failure));
                let mut restored = plugin;
                restored.active = recovery.target.as_ref().map(|(id, _)| *id);
                restored.rollback_target = None;
                let change = ActivationWrite {
                    plugins: vec![restored],
                    failed: recovery.failed,
                    ..ActivationWrite::default()
                };
                commit(deps, state, &change).await?;
                if let Some((id, loaded)) = recovery.target {
                    state.loaded.insert(id.generation, loaded);
                }
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
            let recovery = prepare(&self.deps, candidates).await?;
            if let Some(plugin) = change.plugins.iter_mut().find(|plugin| plugin.id == id) {
                plugin.active = recovery.target.as_ref().map(|(id, _)| *id);
                plugin.rollback_target = None;
            }
            change.failed.extend(recovery.failed);
            if let Some((id, adapter)) = recovery.target {
                loaded.insert(id.generation, adapter);
            }
        }
        let mut state = self.state.lock().await;
        if !expected.matches(&state) {
            return Err(RegistryError::Conflict("rollback".into()));
        }
        let config = change
            .configuration
            .map(|id| {
                let record = state
                    .history
                    .config_generations
                    .get(&id)
                    .ok_or(RegistryError::UnknownGeneration(id))?;
                configuration::declare(&record.document, &self.deps)
                    .map(|declaration| declaration.into_active(record))
                    .map_err(RegistryError::Config)
            })
            .transpose()?;
        commit(&self.deps, &mut state, &change).await?;
        state.loaded.extend(loaded);
        self.publish(&mut state, config, None);
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

fn rollback_plan(state: &State) -> Result<RollbackPlan, RegistryError> {
    let initial = state.history.registrations.keys().next().copied();
    let registration = state
        .history
        .registrations
        .values()
        .rev()
        .find(|record| Some(record.id) != initial && effective(state, record))
        .ok_or(RegistryError::NoRollbackTarget)?;
    let mut change = ActivationWrite::default();
    if let Some(config) = state
        .history
        .active_config
        .and_then(|id| state.history.config_generations.get(&id))
        && config.registration == registration.id
    {
        change.configuration = config.previous;
    }
    let mut recoveries = BTreeMap::new();
    for plugin in state.history.plugins.values() {
        let code = plugin
            .active
            .is_some_and(|active| active.registration == Some(registration.id));
        let name = plugin.named_by == Some(registration.id);
        if !code && !name {
            continue;
        }
        let mut restored = plugin.clone();
        let row = registration.names.get(&plugin.id);
        if name {
            restored.named_by = row.and_then(|row| row.previous);
        }
        let added = name && restored.named_by.is_none();
        let removed = name && row.is_some_and(|row| row.name.is_none());
        if added {
            restored.active = None;
            restored.rollback_target = None;
        } else if code || removed {
            recoveries.insert(plugin.id, candidates(state, plugin));
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
    state
        .history
        .active_config
        .and_then(|id| state.history.config_generations.get(&id))
        .is_some_and(|config| config.registration == record.id)
        || state.history.plugins.values().any(|plugin| {
            plugin.named_by == Some(record.id)
                || plugin
                    .active
                    .is_some_and(|active| active.registration == Some(record.id))
        })
}
