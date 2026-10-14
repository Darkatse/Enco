use super::*;

enum Follow {
    Unchanged,
    Factory,
    Trial,
}

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
        uses: &[Use],
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
            state.register_factory(deps, factory, uses).await?;
        }
        Ok(state)
    }

    async fn register_factory(
        &mut self,
        deps: &RegistryDeps,
        factory: &FactoryPlugin,
        uses: &[Use],
    ) -> Result<(), RegistryError> {
        if !self.names.contains_key(&factory.name) {
            deps.store
                .register_plugin(&factory.name, factory.id)
                .await?;
            self.names.insert(factory.name.clone(), factory.id);
        }
        let hash = deps.store.put_artifact(&factory.artifact).await?;
        let follow = self.follow_factory(deps, factory, hash, uses).await?;
        let activate = matches!(follow, Follow::Factory) || !self.active.contains_key(&factory.id);
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
                    settings: PluginSettings::default(),
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

    async fn follow_factory(
        &mut self,
        deps: &RegistryDeps,
        factory: &FactoryPlugin,
        artifact: ContentHash,
        uses: &[Use],
    ) -> Result<Follow, RegistryError> {
        let previous_factory = self
            .generations
            .values()
            .rev()
            .find(|record| record.plugin == factory.id && record.origin == Origin::Factory);
        let active = self
            .active
            .get(&factory.id)
            .and_then(|id| self.generations.get(id));
        let Some((previous, active)) = previous_factory.zip(active) else {
            return Ok(Follow::Unchanged);
        };
        if active.artifact != previous.artifact || active.artifact == artifact {
            return Ok(Follow::Unchanged);
        }
        if active.settings == PluginSettings::default() {
            return Ok(Follow::Factory);
        }
        let generation = NewGeneration {
            plugin: factory.id,
            artifact,
            settings: active.settings.clone(),
            origin: Origin::Deployed,
            status: GenerationStatus::Trial,
            created_at: deps.clock.now(),
        };
        let candidate = registration::load(
            deps.runtime.as_ref(),
            &factory.name,
            &factory.artifact,
            &generation.settings,
        )
        .await;
        let loaded = match candidate.and_then(|loaded| {
            let before = admission::evaluate(self, &deps.host_tool_names, &BTreeMap::new());
            let candidates = BTreeMap::from([(factory.name.clone(), loaded.clone())]);
            let after = admission::evaluate(self, &deps.host_tool_names, &candidates);
            after.accept_change(&before, std::slice::from_ref(&factory.name), uses, uses)?;
            Ok(loaded)
        }) {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(plugin = factory.name, %error, "factory upgrade rejected; keeping registered content");
                return Ok(Follow::Unchanged);
            }
        };
        // Register the trial before the new recovery floor, so older healthy settings come first.
        let id = deps.store.insert_generation(&generation, true).await?;
        self.registered(generation.numbered(id), loaded);
        Ok(Follow::Trial)
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
    pub fn settings_divergence(&self) -> BTreeMap<String, GenerationId> {
        self.plugins
            .active_records()
            .into_iter()
            .filter_map(|(name, record)| {
                (self.config.plugins.get(&name).cloned().unwrap_or_default() != record.settings)
                    .then_some((name, record.id))
            })
            .collect()
    }

    pub async fn initialize(
        deps: &RegistryDeps,
    ) -> Result<(Self, admission::Admission), RegistryError> {
        let mut records = deps.store.registry().await?;
        let mut configs: BTreeMap<_, _> = std::mem::take(&mut records.config_generations)
            .into_iter()
            .map(|record| (record.id, record))
            .collect();
        let active = records.active_config;
        let document = match active {
            Some(id) => configs
                .get(&id)
                .ok_or(RegistryError::UnknownGeneration(id))?
                .document
                .clone(),
            None => deps.bootstrap.clone().map_err(RegistryError::Bootstrap)?,
        };
        let declaration = if active.is_some() {
            configuration::declare(&document, &deps.owners).map_err(RegistryError::Config)?
        } else {
            configuration::declare_valid(&document, &deps.owners)?
        };
        let mut plugins = Plugins::initialize(deps, records, &declaration.uses).await?;
        activation::restore(deps, &mut plugins).await?;
        let (config, admission) = match active {
            Some(id) => {
                let admission =
                    admission::evaluate(&plugins, &deps.host_tool_names, &BTreeMap::new());
                (
                    declaration.into_active(
                        configs
                            .get(&id)
                            .ok_or(RegistryError::UnknownGeneration(id))?,
                    ),
                    admission,
                )
            }
            None => {
                let prepared = registration::settings(
                    deps.store.as_ref(),
                    deps.runtime.as_ref(),
                    &BTreeMap::new(),
                    &declaration.plugins,
                    &plugins.active_records(),
                    deps.clock.now(),
                )
                .await?;
                let candidates = prepared
                    .iter()
                    .map(|(name, prepared)| (name.clone(), prepared.loaded.clone()))
                    .collect();
                let admission = admission::evaluate(&plugins, &deps.host_tool_names, &candidates);
                // First registration adopts everything in use, so every plugin and use must work.
                let names: Vec<_> = plugins.names.keys().cloned().collect();
                admission.accept_change(&Default::default(), &names, &declaration.uses, &[])?;
                let record = configuration::register(
                    deps.store.as_ref(),
                    &mut configs,
                    &mut plugins,
                    prepared,
                    document,
                    deps.clock.now(),
                )
                .await?;
                (declaration.into_active(&record), admission)
            }
        };
        Ok((
            Self {
                plugins,
                configs,
                config,
            },
            admission,
        ))
    }

    pub fn view(&self, admission: admission::Admission) -> View {
        View {
            exports: self.plugins.exports(admission),
            config: self.config.clone(),
        }
    }
}
