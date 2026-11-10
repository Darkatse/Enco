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
    pub factory: BTreeMap<PluginId, ContentHash>,
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

    /// The artifact a plugin's code runs.
    pub fn artifact(&self, code: CodeRef) -> Result<ContentHash, RegistryError> {
        match code.code {
            PluginCode::Generation { registration } => Ok(self
                .generation(GenerationId {
                    registration,
                    plugin: code.plugin,
                })?
                .artifact),
            PluginCode::Factory { artifact } => Ok(artifact),
        }
    }

    /// The bytes and settings a plugin's code runs: factory code runs with empty settings.
    pub fn content(&self, code: CodeRef) -> Result<(ContentHash, PluginSettings), RegistryError> {
        match code.code {
            PluginCode::Generation { registration } => {
                let record = self.generation(GenerationId {
                    registration,
                    plugin: code.plugin,
                })?;
                Ok((record.artifact, record.settings.clone()))
            }
            PluginCode::Factory { artifact } => Ok((artifact, PluginSettings::default())),
        }
    }

    /// A generation's standing on this node; factory code is never on trial.
    pub fn health(&self, code: CodeRef) -> Option<&Health> {
        self.node.health.get(&code.generation()?)
    }

    /// The one derivation of where a plugin's code came from, for both layers: settings follow
    /// the code they reuse.
    pub fn provenance(&self, code: CodeRef) -> Result<CodeProvenance, RegistryError> {
        let mut code = code;
        loop {
            let Some(id) = code.generation() else {
                return Ok(CodeProvenance {
                    code: code.code,
                    source: CodeSource::Factory,
                });
            };
            let source = match &self.generation(id)?.origin {
                Origin::Settings { based_on } => {
                    code.code = *based_on;
                    continue;
                }
                Origin::Code {
                    intent: CodeIntent::Directory { source },
                } => CodeSource::Directory {
                    registration: id.registration,
                    source: *source,
                },
                Origin::Code {
                    intent: CodeIntent::Factory,
                } => CodeSource::Factory,
            };
            return Ok(CodeProvenance {
                code: code.code,
                source,
            });
        }
    }

    /// A plugin's reference with everything status shows about it.
    pub fn activation(
        &self,
        plugin: PluginId,
        reference: PluginActivation,
    ) -> Result<Activation, RegistryError> {
        let code = CodeRef {
            plugin,
            code: reference.code(),
        };
        let (artifact, settings) = self.content(code)?;
        let origin = code
            .generation()
            .map(|id| self.generation(id).map(|record| record.origin.clone()))
            .transpose()?;
        Ok(Activation {
            provenance: self.provenance(code)?,
            health: self.health(code).cloned(),
            artifact,
            settings,
            origin,
            reference,
        })
    }

    /// The code a plugin runs on this node.
    pub fn active(&self, plugin: PluginId) -> Option<CodeRef> {
        let active = self.node.plugins.get(&plugin)?.active?;
        Some(CodeRef {
            plugin,
            code: active.code(),
        })
    }

    pub fn contents(&self) -> BTreeMap<PluginName, Option<Loaded>> {
        self.running_names()
            .into_iter()
            .map(|(name, id)| {
                let loaded = self
                    .active(id)
                    .and_then(|code| self.loaded.get(&code))
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
    pub fn referenced(&self) -> impl Iterator<Item = CodeRef> {
        self.node.plugins.values().flat_map(|plugin| {
            [plugin.active, plugin.rollback_target]
                .into_iter()
                .flatten()
                .map(|reference| CodeRef {
                    plugin: plugin.id,
                    code: reference.code(),
                })
        })
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
            .chain(self.factory.iter().map(|(plugin, artifact)| CodeRef {
                plugin: *plugin,
                code: PluginCode::Factory {
                    artifact: *artifact,
                },
            }))
            .collect();
        self.loaded.retain(|code, _| retained.contains(code));
        let trials: BTreeSet<_> = self
            .node
            .plugins
            .keys()
            .filter_map(|plugin| self.active(*plugin)?.generation())
            .filter(|id| self.node.health.get(id) == Some(&Health::Trial))
            .collect();
        self.trial_successes.retain(|id, _| trials.contains(id));
    }

    /// This host's factory code for `plugin`, when the host reserves one.
    pub fn factory_code(&self, plugin: PluginId) -> Option<CodeRef> {
        let artifact = *self.factory.get(&plugin)?;
        Some(CodeRef {
            plugin,
            code: PluginCode::Factory { artifact },
        })
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
