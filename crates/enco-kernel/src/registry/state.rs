use super::*;

pub(super) struct Plugins {
    pub names: BTreeMap<String, PluginId>,
    pub generations: BTreeMap<GenerationId, GenerationRecord>,
    pub active: BTreeMap<PluginId, GenerationId>,
    pub factory: BTreeMap<PluginId, GenerationId>,
    pub loaded: BTreeMap<GenerationId, Loaded>,
    pub trial_successes: BTreeMap<GenerationId, u32>,
}

impl Plugins {
    pub async fn initialize(
        deps: &RegistryDeps,
        records: crate::RegistryState,
    ) -> Result<Self, RegistryError> {
        let names = deps.store.plugin_names().await?;
        for name in names.keys() {
            validate_name(name)?;
        }
        for factory in &deps.factory {
            if names.get(&factory.name).is_some_and(|id| id != &factory.id) {
                return Err(RegistryError::FactoryIdentity(factory.name.clone()));
            }
        }
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
                if activate && self.active.get(&factory.id) != Some(&id) {
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
                    created_at: deps.clock.now(),
                };
                let id = deps.store.insert_generation(&record, activate).await?;
                self.generations.insert(id, record.numbered(id));
                id
            }
        };
        if activate {
            self.active.insert(factory.id, id);
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

    /// Earlier healthy generations first, with the current factory as the recovery floor.
    pub fn candidates(&self, plugin: PluginId, before: GenerationId) -> Vec<GenerationRecord> {
        let mut candidates: Vec<_> = self
            .generations
            .range(..before)
            .rev()
            .filter(|(_, record)| {
                record.plugin == plugin && record.status == GenerationStatus::Healthy
            })
            .map(|(_, record)| record.clone())
            .collect();
        if let Some(factory) = self.factory.get(&plugin)
            && *factory != before
            && !candidates.iter().any(|record| record.id == *factory)
            && let Some(record) = self.generations.get(factory)
            && record.status == GenerationStatus::Healthy
        {
            candidates.push(record.clone());
        }
        candidates
    }
}

pub(super) struct State {
    pub plugins: Plugins,
    pub configs: BTreeMap<GenerationId, ConfigGeneration>,
    pub config: ActiveConfig,
}

impl State {
    pub async fn initialize(deps: &RegistryDeps) -> Result<Self, RegistryError> {
        let mut records = deps.store.registry().await?;
        let mut configs: BTreeMap<_, _> = std::mem::take(&mut records.config_generations)
            .into_iter()
            .map(|record| (record.id, record))
            .collect();
        let active = records.active_config;
        let mut plugins = Plugins::initialize(deps, records).await?;
        activation::restore(deps, &mut plugins).await?;
        let config = match active {
            // Restoration is mechanical: it reports what this host cannot honor and keeps running.
            Some(id) => {
                let record = configs
                    .get(&id)
                    .ok_or(RegistryError::UnknownGeneration(id))?;
                let declaration = configuration::declare(&record.document, &deps.owners)
                    .map_err(RegistryError::Config)?;
                if let Err(error) = check_uses(&plugins, &declaration.uses) {
                    tracing::warn!(%error, "restored configuration has unavailable uses");
                }
                declaration.into_active(record)
            }
            // First registration adopts the configuration in use, so it must be fully valid.
            None => {
                let document = deps.bootstrap.clone().map_err(RegistryError::Bootstrap)?;
                let declaration = configuration::declare_valid(&document, &deps.owners)?;
                check_uses(&plugins, &declaration.uses)?;
                let record = configuration::register(
                    deps.store.as_ref(),
                    &mut configs,
                    document,
                    deps.clock.now(),
                )
                .await?;
                declaration.into_active(&record)
            }
        };
        Ok(Self {
            plugins,
            configs,
            config,
        })
    }

    pub fn view(&self) -> View {
        View {
            exports: self.plugins.exports(),
            config: self.config.clone(),
        }
    }
}
