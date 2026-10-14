use super::*;
use state::Plugins;

pub(super) enum ActivationReason {
    Manual,
    Recovery(Failure),
    TrialFailure {
        failure: Failure,
        session: Option<SessionId>,
    },
}

pub(super) struct Activated {
    pub target: Option<GenerationRecord>,
}

struct Prepared {
    failed: Vec<(GenerationId, Failure)>,
    target: Option<(GenerationRecord, Loaded)>,
}

impl Registry {
    /// Prepare without the committer, then check the captured facts before writing.
    pub(super) async fn activate(
        &self,
        name: &str,
        plugin: PluginId,
        expected: GenerationId,
        candidates: Vec<GenerationRecord>,
        reason: ActivationReason,
    ) -> Result<Activated, RegistryError> {
        let cached = self.state.lock().await.plugins.loaded.clone();
        let prepared = prepare(
            self.store.as_ref(),
            self.runtime.as_ref(),
            name,
            candidates,
            &cached,
        )
        .await?;
        let mut state = self.state.lock().await;
        let activated = commit(
            self.store.as_ref(),
            self.clock.as_ref(),
            &mut state.plugins,
            plugin,
            expected,
            prepared,
            reason,
        )
        .await?;
        self.publish(&mut state, None);
        Ok(activated)
    }
}

pub(super) async fn restore(
    deps: &RegistryDeps,
    plugins: &mut Plugins,
) -> Result<(), RegistryError> {
    for (plugin, generation) in plugins.factory.clone() {
        let name = plugins.name_of(plugin)?;
        let record = &plugins.generations[&generation];
        let loaded = load(deps.store.as_ref(), deps.runtime.as_ref(), name, record).await?;
        plugins.loaded.insert(generation, loaded);
    }
    for (plugin, generation) in plugins.active.clone() {
        if plugins.loaded.contains_key(&generation) {
            continue;
        }
        let name = plugins.name_of(plugin)?.to_owned();
        let record = &plugins.generations[&generation];
        match load(deps.store.as_ref(), deps.runtime.as_ref(), &name, record).await {
            Ok(loaded) => {
                plugins.loaded.insert(generation, loaded);
            }
            Err(error) => {
                let failure = load_failure(error)?;
                let candidates = plugins.candidates(plugin, generation);
                let prepared = prepare(
                    deps.store.as_ref(),
                    deps.runtime.as_ref(),
                    &name,
                    candidates,
                    &plugins.loaded,
                )
                .await?;
                commit(
                    deps.store.as_ref(),
                    deps.clock.as_ref(),
                    plugins,
                    plugin,
                    generation,
                    prepared,
                    ActivationReason::Recovery(failure),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn load(
    store: &dyn Store,
    runtime: &dyn Runtime,
    name: &str,
    record: &GenerationRecord,
) -> Result<Loaded, RegistryError> {
    let bytes = store.artifact(&record.artifact).await?;
    runtime
        .load(&bytes, &record.settings)
        .await
        .map_err(|error| RegistryError::Rejected {
            name: name.into(),
            reason: error.to_string(),
        })
}

async fn prepare(
    store: &dyn Store,
    runtime: &dyn Runtime,
    name: &str,
    candidates: Vec<GenerationRecord>,
    cached: &BTreeMap<GenerationId, Loaded>,
) -> Result<Prepared, RegistryError> {
    let mut prepared = Prepared {
        failed: vec![],
        target: None,
    };
    for record in candidates {
        let result = match cached.get(&record.id) {
            Some(loaded) => Ok(loaded.clone()),
            None => load(store, runtime, name, &record).await,
        };
        match result {
            Ok(loaded) => {
                prepared.target = Some((record, loaded));
                break;
            }
            Err(error) => prepared.failed.push((record.id, load_failure(error)?)),
        }
    }
    Ok(prepared)
}

async fn commit(
    store: &dyn Store,
    clock: &dyn Clock,
    plugins: &mut Plugins,
    plugin: PluginId,
    expected: GenerationId,
    prepared: Prepared,
    reason: ActivationReason,
) -> Result<Activated, RegistryError> {
    let name = plugins.name_of(plugin)?.to_owned();
    if plugins.active.get(&plugin) != Some(&expected)
        || (matches!(reason, ActivationReason::TrialFailure { .. })
            && plugins
                .generations
                .get(&expected)
                .is_none_or(|record| record.status != GenerationStatus::Trial))
        || prepared.target.as_ref().is_some_and(|(target, _)| {
            plugins
                .generations
                .get(&target.id)
                .is_none_or(|record| record.status != GenerationStatus::Healthy)
        })
    {
        return Err(RegistryError::Conflict(name));
    }
    let mut failed = prepared.failed;
    match &reason {
        ActivationReason::Manual => {}
        ActivationReason::Recovery(failure) | ActivationReason::TrialFailure { failure, .. } => {
            failed.push((expected, failure.clone()))
        }
    }
    let to = match &prepared.target {
        Some((record, _)) => Some(record.id),
        None if matches!(reason, ActivationReason::Manual) => Some(expected),
        None => None,
    };
    let event = match reason {
        ActivationReason::TrialFailure {
            failure,
            session: Some(session),
        } => {
            let rollback = RolledBack {
                plugin: name.clone(),
                from: expected,
                to,
                failure,
            };
            Some(Event {
                id: EventId::new(),
                session,
                source: EventSource::Registry,
                body: EventBody::GenerationRolledBack {
                    text: notice(&rollback),
                    rollback,
                },
                received_at: clock.now(),
            })
        }
        ActivationReason::TrialFailure { session: None, .. }
        | ActivationReason::Manual
        | ActivationReason::Recovery(_) => None,
    };
    store.activate(plugin, to, &failed, event.as_ref()).await?;
    for (id, failure) in failed {
        tracing::warn!(plugin = name, generation = %id, code = %failure.code, reason = %failure.message, "generation marked failed");
        if let Some(record) = plugins.generations.get_mut(&id) {
            record.status = GenerationStatus::Failed;
            record.failure = Some(failure);
        }
    }
    match to {
        Some(id) => {
            plugins.active.insert(plugin, id);
        }
        None => {
            plugins.active.remove(&plugin);
        }
    }
    let target = prepared.target.map(|(record, loaded)| {
        plugins.loaded.insert(record.id, loaded);
        record
    });
    Ok(Activated { target })
}

fn load_failure(error: RegistryError) -> Result<Failure, RegistryError> {
    match error {
        RegistryError::Store(StoreError::Artifact(_)) | RegistryError::Rejected { .. } => {
            Ok(Failure {
                code: code::PLUGIN_LOAD.into(),
                message: error.to_string(),
                retryable: false,
            })
        }
        _ => Err(error),
    }
}

fn notice(rollback: &RolledBack) -> String {
    let target = match rollback.to {
        Some(to) => format!("now using generation {to}"),
        None => "no generation is active".into(),
    };
    format!(
        "[plugin rolled back] {}: generation {} failed ({}: {}); {target}",
        rollback.plugin, rollback.from, rollback.failure.code, rollback.failure.message
    )
}
