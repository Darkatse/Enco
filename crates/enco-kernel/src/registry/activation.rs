use super::*;

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
    pub rollback: Option<RolledBack>,
}

impl Registry {
    /// Startup is unpublished until all restoration and admission steps finish.
    pub(super) async fn restore(&self) -> Result<(), RegistryError> {
        let (factory, active) = {
            let state = self.state.lock().await;
            (state.factory.clone(), state.active.clone())
        };
        for (plugin, generation) in factory {
            let (name, record) = self.record(plugin, generation).await?;
            let loaded = self.load(&name, &record).await?;
            self.state.lock().await.loaded.insert(generation, loaded);
        }
        for (plugin, generation) in active {
            let Some(generation) = generation else {
                continue;
            };
            if self.state.lock().await.loaded.contains_key(&generation) {
                continue;
            }
            let (name, record) = self.record(plugin, generation).await?;
            match self.load(&name, &record).await {
                Ok(loaded) => {
                    self.state.lock().await.loaded.insert(generation, loaded);
                }
                Err(error) => {
                    let failure = load_failure(error)?;
                    let candidates = self.state.lock().await.candidates(plugin, generation);
                    self.activate(
                        &name,
                        plugin,
                        generation,
                        candidates,
                        ActivationReason::Recovery(failure),
                    )
                    .await?;
                }
            }
        }
        let mut state = self.state.lock().await;
        for usage in &self.wiring {
            let plugin = state
                .names
                .get(&usage.plugin)
                .ok_or_else(|| RegistryError::Rejected {
                    name: usage.plugin.clone(),
                    reason: format!("used by {}; name is not registered", usage.user),
                })?;
            let loaded = state
                .active
                .get(plugin)
                .copied()
                .flatten()
                .and_then(|generation| state.loaded.get(&generation))
                .ok_or_else(|| RegistryError::Rejected {
                    name: usage.plugin.clone(),
                    reason: format!("used by {}; no active generation", usage.user),
                })?;
            self.admit(&usage.plugin, loaded)?;
        }
        self.publish(&mut state);
        Ok(())
    }

    async fn record(
        &self,
        plugin: PluginId,
        generation: GenerationId,
    ) -> Result<(String, GenerationRecord), RegistryError> {
        let state = self.state.lock().await;
        let name = state.name_of(plugin)?.to_owned();
        let record = state
            .generations
            .get(&generation)
            .cloned()
            .ok_or(StoreError::UnknownGeneration(generation))?;
        Ok((name, record))
    }

    async fn load(&self, name: &str, record: &GenerationRecord) -> Result<Loaded, RegistryError> {
        let bytes = self.store.artifact(&record.artifact).await?;
        self.runtime
            .load(&bytes, &record.config)
            .await
            .map_err(|error| RegistryError::Rejected {
                name: name.into(),
                reason: error.to_string(),
            })
    }

    /// Preparation never writes state. The caller captured expected and candidates together.
    pub(super) async fn activate(
        &self,
        name: &str,
        plugin: PluginId,
        expected: GenerationId,
        candidates: Vec<GenerationRecord>,
        reason: ActivationReason,
    ) -> Result<Activated, RegistryError> {
        let mut failed = match &reason {
            ActivationReason::Manual => vec![],
            ActivationReason::Recovery(failure)
            | ActivationReason::TrialFailure { failure, .. } => {
                vec![(expected, failure.clone())]
            }
        };
        let mut prepared = None;
        for record in candidates {
            let cached = self.state.lock().await.loaded.get(&record.id).cloned();
            let result = match cached {
                Some(loaded) => Ok(loaded),
                None => self.load(name, &record).await,
            };
            match result {
                Ok(loaded) => {
                    prepared = Some((record, loaded));
                    break;
                }
                Err(error) => failed.push((record.id, load_failure(error)?)),
            }
        }
        let mut state = self.state.lock().await;
        if state.active.get(&plugin) != Some(&Some(expected))
            || (matches!(reason, ActivationReason::TrialFailure { .. })
                && state
                    .generations
                    .get(&expected)
                    .is_none_or(|record| record.status != GenerationStatus::Trial))
            || prepared.as_ref().is_some_and(|(target, _)| {
                state
                    .generations
                    .get(&target.id)
                    .is_none_or(|record| record.status != GenerationStatus::Healthy)
            })
        {
            return Err(RegistryError::Conflict(name.into()));
        }
        let to = match &prepared {
            Some((record, _)) => Some(record.id),
            // Manual rollback keeps the working generation when no usable target remains.
            None if matches!(reason, ActivationReason::Manual) => Some(expected),
            None => None,
        };
        let (rollback, event) = match reason {
            ActivationReason::TrialFailure { failure, session } => {
                let rollback = RolledBack {
                    plugin: name.into(),
                    from: expected,
                    to,
                    failure,
                };
                let event = session.map(|session| Event {
                    id: EventId::new(),
                    session,
                    source: EventSource::Registry,
                    body: EventBody::GenerationRolledBack(rollback.clone()),
                    received_at: self.clock.now().to_utc(),
                });
                (Some(rollback), event)
            }
            ActivationReason::Manual | ActivationReason::Recovery(_) => (None, None),
        };
        self.store
            .activate(plugin, to, &failed, event.as_ref())
            .await?;
        for (id, failure) in failed {
            tracing::warn!(plugin = name, generation = %id, code = %failure.code, reason = %failure.message, "generation marked failed");
            if let Some(record) = state.generations.get_mut(&id) {
                record.status = GenerationStatus::Failed;
                record.failure = Some(failure);
            }
        }
        state.active.insert(plugin, to);
        let target = prepared.map(|(record, loaded)| {
            state.loaded.insert(record.id, loaded);
            record
        });
        self.publish(&mut state);
        Ok(Activated { target, rollback })
    }
}

/// Only unavailable artifacts or runtime rejection belong to the generation.
/// Storage faults must stop the owner without changing plugin health.
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
