use super::*;

pub(super) struct State {
    pub names: BTreeMap<String, PluginId>,
    pub generations: BTreeMap<GenerationId, GenerationRecord>,
    pub active: BTreeMap<PluginId, Option<GenerationId>>,
    pub factory: BTreeMap<PluginId, GenerationId>,
    pub loaded: BTreeMap<GenerationId, Loaded>,
    pub trial_successes: BTreeMap<GenerationId, u32>,
}

impl State {
    pub async fn read(deps: &RegistryDeps) -> Result<Self, RegistryError> {
        let names = deps.store.plugin_names().await?;
        for name in names.keys() {
            validate_name(name)?;
        }
        for factory in &deps.factory {
            if names.get(&factory.name).is_some_and(|id| id != &factory.id) {
                return Err(RegistryError::FactoryIdentity(factory.name.clone()));
            }
        }
        let records = deps.store.registry().await?;
        for record in &records.generations {
            if !names.values().any(|id| *id == record.plugin) {
                return Err(RegistryError::UnknownPlugin(record.plugin.to_string()));
            }
        }
        let mut state = Self {
            names,
            generations: records
                .generations
                .into_iter()
                .map(|record| (record.id, record))
                .collect(),
            active: records.active,
            factory: BTreeMap::new(),
            loaded: BTreeMap::new(),
            trial_successes: BTreeMap::new(),
        };
        for factory in &deps.factory {
            state.register_factory(deps, factory).await?;
        }
        Ok(state)
    }

    async fn register_factory(
        &mut self,
        deps: &RegistryDeps,
        factory: &FactoryPlugin,
    ) -> Result<(), RegistryError> {
        if !self.names.contains_key(&factory.name) {
            deps.store
                .register_plugin(&factory.name, factory.id)
                .await?;
            self.names.insert(factory.name.clone(), factory.id);
        }
        let hash = deps.store.put_artifact(&factory.artifact).await?;
        let active = self
            .active
            .get(&factory.id)
            .copied()
            .flatten()
            .and_then(|id| self.generations.get(&id));
        let activate = active.is_none_or(|record| record.origin == Origin::Factory);
        let existing = self
            .generations
            .values()
            .rev()
            .find(|record| {
                record.plugin == factory.id
                    && record.artifact == hash
                    && record.origin == Origin::Factory
                    && record.status == GenerationStatus::Healthy
            })
            .map(|record| record.id);
        let id = match existing {
            Some(id) => {
                if activate && self.active.get(&factory.id) != Some(&Some(id)) {
                    deps.store.activate(factory.id, Some(id), &[], None).await?;
                }
                id
            }
            None => {
                let record = NewGeneration {
                    plugin: factory.id,
                    artifact: hash,
                    config: serde_json::json!({}),
                    origin: Origin::Factory,
                    status: GenerationStatus::Healthy,
                    created_at: deps.clock.now().to_utc(),
                };
                let id = deps.store.insert_generation(&record, activate).await?;
                self.generations.insert(id, record.numbered(id));
                id
            }
        };
        if activate {
            self.active.insert(factory.id, Some(id));
        }
        self.factory.insert(factory.id, id);
        Ok(())
    }

    pub fn name_of(&self, plugin: PluginId) -> Result<&str, RegistryError> {
        self.names
            .iter()
            .find(|(_, id)| **id == plugin)
            .map(|(name, _)| name.as_str())
            .ok_or_else(|| RegistryError::UnknownPlugin(plugin.to_string()))
    }

    /// Candidates strictly descend in commit order; exhaustion does not require a factory plugin.
    pub fn candidates(&self, plugin: PluginId, before: GenerationId) -> Vec<GenerationRecord> {
        self.generations
            .range(..before)
            .rev()
            .filter(|(_, record)| {
                record.plugin == plugin && record.status == GenerationStatus::Healthy
            })
            .map(|(_, record)| record.clone())
            .collect()
    }

    pub fn exports(&self) -> Exports {
        Exports {
            generations: self
                .generations
                .iter()
                .map(|(id, record)| (*id, record.plugin))
                .collect(),
            plugins: self
                .names
                .iter()
                .map(|(name, plugin)| {
                    let export = |generation: GenerationId| {
                        self.loaded.get(&generation).map(|loaded| LoadedGeneration {
                            plugin: *plugin,
                            generation,
                            loaded: loaded.clone(),
                        })
                    };
                    (
                        name.clone(),
                        Routes {
                            active: self.active.get(plugin).copied().flatten().and_then(export),
                            factory: self.factory.get(plugin).copied().and_then(export),
                        },
                    )
                })
                .collect(),
        }
    }
}
