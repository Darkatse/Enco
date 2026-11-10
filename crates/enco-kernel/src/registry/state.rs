use super::*;
use crate::{AcceptedIntent, ActivationWrite, NodeState, RegistrationWrite, RegistryState};
use std::collections::BTreeSet;

/// Persisted references own lifetime; loaded adapters and names are projections.
pub(super) struct State {
    /// What was registered, the same on every node.
    pub intent: AcceptedIntent,
    /// What runs on this node and how it fared.
    pub node: NodeState,
    /// Compiled code for every retained reference and this host's factory code.
    pub loaded: BTreeMap<CodeRef, Loaded>,
    /// This host's factory code, by the plugin it is reserved for.
    pub factory: BTreeMap<PluginId, CodeRef>,
    pub trial_successes: BTreeMap<GenerationId, u32>,
}

impl State {
    pub fn new(history: RegistryState) -> Self {
        Self {
            intent: history.intent,
            node: history.node,
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
        self.intent
            .registrations
            .get(&registration?)?
            .names
            .get(&plugin)
    }

    pub fn running_name(&self, plugin: PluginId) -> Option<&PluginName> {
        let record = self.node.plugins.get(&plugin)?;
        record.active.as_ref()?;
        self.name_at(plugin, record.named_by)?.name.as_ref()
    }

    pub fn running_names(&self) -> BTreeMap<PluginName, PluginId> {
        self.node
            .plugins
            .keys()
            .filter_map(|id| self.running_name(*id).map(|name| (name.to_owned(), *id)))
            .collect()
    }

    /// Accepted names remain distinct from names changed by mechanical recovery.
    pub fn registered_names(&self) -> BTreeMap<PluginName, PluginId> {
        self.registered_names_at(None)
    }

    pub fn registrations_through(
        &self,
        at: Option<RegistrationId>,
    ) -> std::collections::btree_map::Range<'_, RegistrationId, RegistrationRecord> {
        self.intent.registrations.range((
            std::ops::Bound::Unbounded,
            at.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Included),
        ))
    }

    pub fn registered_name(
        &self,
        plugin: PluginId,
        at: Option<RegistrationId>,
    ) -> Option<(RegistrationId, &RegisteredName)> {
        self.registrations_through(at)
            .rev()
            .find_map(|(id, registration)| registration.names.get(&plugin).map(|name| (*id, name)))
    }

    pub fn registered_names_at(
        &self,
        at: Option<RegistrationId>,
    ) -> BTreeMap<PluginName, PluginId> {
        let mut names = BTreeMap::new();
        for (_, registration) in self.registrations_through(at) {
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
        self.registered_config_at(None)
    }

    pub fn registered_config_at(&self, at: Option<RegistrationId>) -> Option<&ConfigGeneration> {
        let (last, _) = self.registrations_through(at).next_back()?;
        self.intent
            .configurations
            .range(..=*last)
            .next_back()
            .map(|(_, config)| config)
    }

    /// The configuration that runs on this node.
    pub fn active_config(&self) -> Option<&ConfigGeneration> {
        self.intent.configurations.get(&self.node.configuration?)
    }

    /// A registered generation's content.
    pub fn generation(&self, id: GenerationId) -> Result<&GenerationRecord, RegistryError> {
        self.intent
            .generations
            .get(&id)
            .ok_or(RegistryError::UnknownGeneration(id))
    }

    /// The artifact a reference runs; only plugin code has one.
    pub fn artifact(&self, code: &CodeRef) -> Result<ContentHash, RegistryError> {
        match code {
            CodeRef::Generation { id } => Ok(self.generation(*id)?.artifact),
            CodeRef::Factory { artifact, .. } => Ok(*artifact),
            CodeRef::Native { name, .. } => Err(RegistryError::Intent(format!(
                "native capability {name} is not plugin code"
            ))),
        }
    }

    /// The bytes and settings a reference runs: factory code runs with empty settings.
    pub fn content(&self, code: &CodeRef) -> Result<(ContentHash, PluginSettings), RegistryError> {
        let settings = match code {
            CodeRef::Generation { id } => self.generation(*id)?.settings.clone(),
            CodeRef::Factory { .. } | CodeRef::Native { .. } => PluginSettings::default(),
        };
        Ok((self.artifact(code)?, settings))
    }

    /// A generation's standing on this node; factory code is never on trial.
    pub fn health(&self, code: &CodeRef) -> Option<&Health> {
        self.node.health.get(&code.generation()?)
    }

    /// The one derivation of where a reference's code came from, for both layers: settings
    /// follow the code they reuse.
    pub fn provenance(&self, code: &CodeRef) -> Result<CodeProvenance, RegistryError> {
        let mut code = code.clone();
        loop {
            let id = match &code {
                CodeRef::Generation { id } => *id,
                CodeRef::Factory { .. } => {
                    return Ok(CodeProvenance {
                        code,
                        source: CodeSource::Factory,
                    });
                }
                CodeRef::Native { name, .. } => {
                    return Err(RegistryError::Intent(format!(
                        "native capability {name} is not plugin code"
                    )));
                }
            };
            match &self.generation(id)?.origin {
                Origin::Settings { based_on } => code = based_on.clone(),
                Origin::Code => {
                    // The registration that brings code always records its intent.
                    let source = match self
                        .intent
                        .registrations
                        .get(&id.registration)
                        .and_then(|registration| registration.code.get(&id.plugin))
                        .and_then(|registered| registered.intent)
                    {
                        Some(CodeIntent::Directory { source }) => CodeSource::Directory {
                            registration: id.registration,
                            source,
                        },
                        Some(CodeIntent::Factory) => CodeSource::Factory,
                        None => {
                            return Err(RegistryError::Intent(format!(
                                "{id} brings code without a code intent"
                            )));
                        }
                    };
                    return Ok(CodeProvenance { code, source });
                }
            }
        }
    }

    pub fn activation(&self, reference: &PluginActivation) -> Result<Activation, RegistryError> {
        let (artifact, settings) = self.content(&reference.code)?;
        let origin = reference
            .code
            .generation()
            .map(|id| self.generation(id).map(|record| record.origin.clone()))
            .transpose()?;
        Ok(Activation {
            provenance: self.provenance(&reference.code)?,
            health: self.health(&reference.code).cloned(),
            artifact,
            settings,
            origin,
            code: reference.code.clone(),
            registration: reference.registration,
        })
    }

    pub fn active(&self, plugin: PluginId) -> Option<&CodeRef> {
        self.node
            .plugins
            .get(&plugin)?
            .active
            .as_ref()
            .map(|active| &active.code)
    }

    pub fn contents(&self) -> BTreeMap<PluginName, Option<Loaded>> {
        self.running_names()
            .into_iter()
            .map(|(name, id)| {
                let loaded = self
                    .active(id)
                    .and_then(|code| self.loaded.get(code))
                    .cloned();
                (name, loaded)
            })
            .collect()
    }

    pub fn next_registration(&self) -> RegistrationId {
        RegistrationId(
            self.intent
                .registrations
                .keys()
                .next_back()
                .map_or(1, |id| id.0 + 1),
        )
    }

    pub fn registered(&mut self, write: RegistrationWrite, loaded: BTreeMap<CodeRef, Loaded>) {
        let registration = write.registration.id;
        self.intent
            .registrations
            .insert(registration, write.registration);
        for record in write.generations {
            self.node.health.insert(record.id, Health::Trial);
            self.intent.generations.insert(record.id, record);
        }
        self.node
            .plugins
            .extend(write.plugins.into_iter().map(|record| (record.id, record)));
        if let Some(config) = write.configuration {
            self.node.configuration = Some(config.registration);
            self.intent
                .configurations
                .insert(config.registration, config);
        }
        self.node.replaced.insert(registration, write.replaced);
        self.loaded.extend(loaded);
    }

    pub fn activated(&mut self, change: &ActivationWrite) {
        self.node.plugins.extend(
            change
                .plugins
                .iter()
                .cloned()
                .map(|record| (record.id, record)),
        );
        if let Some(config) = change.configuration {
            self.node.configuration = Some(config);
        }
        for (id, failure) in &change.failed {
            self.node.health.insert(
                *id,
                Health::Failed {
                    failure: failure.clone(),
                },
            );
        }
    }

    /// Code every reference on this node retains: current and rollback targets.
    fn referenced(&self) -> impl Iterator<Item = &CodeRef> {
        self.node
            .plugins
            .values()
            .flat_map(|plugin| [&plugin.active, &plugin.rollback_target])
            .flatten()
            .map(|reference| &reference.code)
    }

    /// The artifacts every reference on this node runs, which artifact cleanup keeps.
    pub fn retained_artifacts(
        &self,
    ) -> Result<std::collections::HashSet<ContentHash>, RegistryError> {
        self.referenced().map(|code| self.artifact(code)).collect()
    }

    pub fn release(&mut self) {
        let retained: BTreeSet<_> = self
            .referenced()
            .chain(self.factory.values())
            .cloned()
            .collect();
        self.loaded.retain(|code, _| retained.contains(code));
        let trials: BTreeSet<_> = self
            .node
            .plugins
            .values()
            .filter_map(|plugin| plugin.active.as_ref()?.code.generation())
            .filter(|id| self.node.health.get(id) == Some(&Health::Trial))
            .collect();
        self.trial_successes.retain(|id, _| trials.contains(id));
    }

    pub fn view(
        &self,
        config: ActiveConfig,
        admission: admission::Admission,
        factory: &[FactoryPlugin],
        composer: &PluginName,
    ) -> Result<View, RegistryError> {
        Ok(View {
            config,
            exports: self.exports(admission, factory, composer)?,
        })
    }
}
