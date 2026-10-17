use super::*;
use crate::{Activation, RegistrationWrite, RegistryState};
use std::collections::BTreeSet;

/// Persisted references own lifetime; loaded adapters and names are projections.
pub(super) struct State {
    pub history: RegistryState,
    pub loaded: BTreeMap<GenerationId, Loaded>,
    pub factory: BTreeMap<PluginId, GenerationId>,
    pub trial_successes: BTreeMap<GenerationId, u32>,
}

impl State {
    pub fn new(history: RegistryState) -> Self {
        Self {
            history,
            loaded: BTreeMap::new(),
            factory: BTreeMap::new(),
            trial_successes: BTreeMap::new(),
        }
    }

    pub fn name_at(
        &self,
        plugin: PluginId,
        registration: Option<RegistrationId>,
    ) -> Option<&RegisteredName> {
        self.history
            .registrations
            .get(&registration?)?
            .names
            .get(&plugin)
    }

    pub fn running_name(&self, plugin: PluginId) -> Option<&str> {
        let record = self.history.plugins.get(&plugin)?;
        record.active?;
        self.name_at(plugin, record.named_by)?.name.as_deref()
    }

    pub fn running_names(&self) -> BTreeMap<String, PluginId> {
        self.history
            .plugins
            .keys()
            .filter_map(|id| self.running_name(*id).map(|name| (name.to_owned(), *id)))
            .collect()
    }

    /// Accepted names remain distinct from names changed by mechanical recovery.
    pub fn registered_names(&self) -> BTreeMap<String, PluginId> {
        let mut names = BTreeMap::new();
        for registration in self.history.registrations.values() {
            for (id, registered) in &registration.names {
                names.retain(|_, plugin| plugin != id);
                if let Some(name) = &registered.name {
                    names.insert(name.clone(), *id);
                }
            }
        }
        names
    }

    pub fn registered_config(&self) -> Option<&ConfigGeneration> {
        self.history.config_generations.values().next_back()
    }

    /// Settings preserve their exact code provenance, even when bytes match another generation.
    pub fn code_generation(&self, mut generation: GenerationId) -> GenerationId {
        while let Origin::Settings {
            based_on: previous, ..
        } = self.history.generations[&generation].origin
        {
            generation = previous;
        }
        generation
    }

    pub fn active(&self, plugin: PluginId) -> Option<&GenerationRecord> {
        self.history
            .generations
            .get(&self.history.plugins.get(&plugin)?.active?.generation)
    }

    pub fn contents(&self) -> BTreeMap<String, Option<Loaded>> {
        self.running_names()
            .into_iter()
            .map(|(name, id)| {
                let loaded = self
                    .active(id)
                    .and_then(|record| self.loaded.get(&record.id))
                    .cloned();
                (name, loaded)
            })
            .collect()
    }

    pub fn next_registration(&self) -> RegistrationId {
        RegistrationId(
            self.history
                .registrations
                .keys()
                .next_back()
                .map_or(1, |id| id.0 + 1),
        )
    }

    pub fn next_generation(&self) -> GenerationId {
        GenerationId(
            self.history
                .generations
                .keys()
                .chain(self.history.config_generations.keys())
                .map(|id| id.0)
                .max()
                .map_or(1, |id| id + 1),
        )
    }

    pub fn registered(&mut self, write: RegistrationWrite, loaded: BTreeMap<GenerationId, Loaded>) {
        self.history
            .registrations
            .insert(write.registration.id, write.registration);
        self.history.generations.extend(
            write
                .generations
                .into_iter()
                .map(|record| (record.id, record)),
        );
        self.history
            .plugins
            .extend(write.plugins.into_iter().map(|record| (record.id, record)));
        if let Some(config) = write.configuration {
            self.history.active_config = Some(config.id);
            self.history.config_generations.insert(config.id, config);
        }
        self.loaded.extend(loaded);
    }

    pub fn activated(&mut self, change: &Activation) {
        self.history.plugins.extend(
            change
                .plugins
                .iter()
                .cloned()
                .map(|record| (record.id, record)),
        );
        if let Some(config) = change.configuration {
            self.history.active_config = Some(config);
        }
        for (id, failure) in &change.failed {
            if let Some(record) = self.history.generations.get_mut(id) {
                record.status = GenerationStatus::Failed;
                record.failure = Some(failure.clone());
            }
        }
    }

    pub fn retained_artifacts(&self) -> std::collections::HashSet<ContentHash> {
        self.history
            .plugins
            .values()
            .flat_map(|plugin| [plugin.active, plugin.rollback_target])
            .flatten()
            .map(|reference| reference.generation)
            .filter_map(|id| self.history.generations.get(&id))
            .map(|record| record.artifact)
            .collect()
    }

    pub fn release(&mut self) {
        let retained: BTreeSet<_> = self
            .history
            .plugins
            .values()
            .flat_map(|plugin| [plugin.active, plugin.rollback_target])
            .flatten()
            .map(|reference| reference.generation)
            .chain(self.factory.values().copied())
            .collect();
        self.loaded.retain(|id, _| retained.contains(id));
        self.trial_successes.retain(|id, _| {
            self.history.generations.get(id).is_some_and(|record| {
                record.status == GenerationStatus::Trial
                    && self
                        .history
                        .plugins
                        .get(&record.plugin)
                        .is_some_and(|plugin| {
                            plugin.active.is_some_and(|active| active.generation == *id)
                        })
            })
        });
    }

    pub fn view(
        &self,
        config: ActiveConfig,
        admission: admission::Admission,
        factory: &[FactoryPlugin],
    ) -> View {
        View {
            config,
            exports: self.exports(admission, factory),
        }
    }
}
