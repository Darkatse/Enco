use super::*;

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
                Err(error) if is_load_failure(&error) => {
                    tracing::warn!(plugin = %name, %generation, %error, "active generation could not load; recovering");
                    let candidates = self.state.lock().await.candidates(plugin, generation);
                    self.activate(&name, plugin, generation, candidates, Some(generation))
                        .await?;
                }
                Err(error) => return Err(error),
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
        let name = state
            .names
            .iter()
            .find(|(_, id)| **id == plugin)
            .map(|(name, _)| name.clone())
            .ok_or_else(|| RegistryError::UnknownPlugin(plugin.to_string()))?;
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
        failed_active: Option<GenerationId>,
    ) -> Result<Option<GenerationRecord>, RegistryError> {
        let mut failed: Vec<_> = failed_active.into_iter().collect();
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
                Err(error) if is_load_failure(&error) => {
                    tracing::warn!(plugin = name, generation = %record.id, %error, "rollback candidate could not load");
                    failed.push(record.id);
                }
                Err(error) => return Err(error),
            }
        }
        let mut state = self.state.lock().await;
        if state.active.get(&plugin) != Some(&Some(expected))
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
            None if failed_active.is_none() => Some(expected),
            None => None,
        };
        self.store.activate(plugin, to, &failed).await?;
        for id in failed {
            if let Some(record) = state.generations.get_mut(&id) {
                record.status = GenerationStatus::Failed;
            }
        }
        state.active.insert(plugin, to);
        let result = prepared.map(|(record, loaded)| {
            state.loaded.insert(record.id, loaded);
            record
        });
        self.publish(&mut state);
        Ok(result)
    }
}

fn is_load_failure(error: &RegistryError) -> bool {
    matches!(
        error,
        RegistryError::Store(StoreError::Artifact(_)) | RegistryError::Rejected { .. }
    )
}
