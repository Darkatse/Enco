//! The registered intent used by management and inspection, independent of running references.
use super::*;
use std::collections::BTreeSet;

/// Only the facts needed to compare and prepare new intent.
#[derive(Clone)]
pub struct RegisteredIntent {
    /// Absent only while preparing the first registration.
    pub base: Option<RegistrationBase>,
    /// Accepted and host-reserved names with their immutable identities.
    pub plugins: BTreeMap<PluginName, RegisteredPlugin>,
}

/// An accepted history position always has a complete configuration.
#[derive(Clone)]
pub struct RegistrationBase {
    /// Latest registration included in this view.
    pub id: RegistrationId,
    /// Configuration selected by the latest configuration registration in the view.
    pub configuration: ConfigGeneration,
}

impl RegisteredIntent {
    /// The accepted position, absent only during bootstrap preparation.
    pub fn registration(&self) -> Option<RegistrationId> {
        self.base.as_ref().map(|base| base.id)
    }

    /// The accepted configuration, absent only during bootstrap preparation.
    pub fn configuration(&self) -> Option<&ConfigGeneration> {
        self.base.as_ref().map(|base| &base.configuration)
    }

    /// Require an accepted view for inspection or restoration.
    pub fn accepted(&self) -> Result<&RegistrationBase, RegistryError> {
        self.base.as_ref().ok_or(RegistryError::NoRegistrations)
    }
    /// Names are not rewound: each plugin keeps the directory of its latest name.
    pub fn rewind_scope(&self, target: &Self) -> RewindScope {
        let mut directories = BTreeMap::new();
        let mut addresses = BTreeMap::new();
        // The latest state is applied last, so its names win.
        for intent in [target, self] {
            for (name, plugin) in &intent.plugins {
                directories.insert(plugin.id, name.clone());
                addresses.insert(name.clone(), plugin.id);
            }
        }
        let changed = directories
            .keys()
            .copied()
            .filter(|id| self.source_of(*id) != target.source_of(*id))
            .collect();
        RewindScope {
            directories,
            addresses,
            changed,
            configuration: self.configuration().map(|config| config.source)
                != target.configuration().map(|config| config.source),
        }
    }

    fn source_of(&self, id: PluginId) -> Option<ContentHash> {
        self.plugin_by_id(id)
            .and_then(|(_, plugin)| plugin.source())
    }

    /// The name and intent of a plugin identity.
    pub fn plugin_by_id(&self, id: PluginId) -> Option<(&PluginName, &RegisteredPlugin)> {
        self.plugins.iter().find(|(_, plugin)| plugin.id == id)
    }
}

/// The paths a rewind between two accepted states can address, decided once for every caller.
pub struct RewindScope {
    /// The directory each identity is written to: its latest name, else its name at the target.
    directories: BTreeMap<PluginId, PluginName>,
    /// The identity a requested name addresses: the latest holder of the name, else the target's.
    addresses: BTreeMap<PluginName, PluginId>,
    changed: BTreeSet<PluginId>,
    configuration: bool,
}

/// The configuration file and plugin directories one rewind writes.
#[derive(Default)]
pub struct RewindSelection {
    /// Whether `config.toml` is written.
    pub configuration: bool,
    /// Each selected identity and the directory it is written to.
    pub plugins: BTreeMap<PluginId, PluginName>,
}

/// A requested rewind that the scope cannot address.
#[derive(Debug)]
pub enum ScopeError {
    /// Neither state has a plugin under the name.
    Unknown(PluginName),
    /// Two selected identities would be written to the same directory.
    Collision(PluginName),
}

impl RewindScope {
    /// Every path whose registered value differs.
    pub fn changed(&self) -> Result<RewindSelection, ScopeError> {
        self.checked(RewindSelection {
            configuration: self.configuration,
            plugins: self
                .changed
                .iter()
                .map(|id| (*id, self.directories[id].clone()))
                .collect(),
        })
    }

    /// Exactly the listed paths, each addressed by its name.
    pub fn select(&self, paths: &[IntentPath]) -> Result<RewindSelection, ScopeError> {
        let mut selection = RewindSelection::default();
        for path in paths {
            match path {
                IntentPath::Configuration => selection.configuration = true,
                IntentPath::Plugin(name) => {
                    let id = *self
                        .addresses
                        .get(name)
                        .ok_or_else(|| ScopeError::Unknown(name.clone()))?;
                    selection.plugins.insert(id, self.directories[&id].clone());
                }
            }
        }
        self.checked(selection)
    }

    /// The directory of an identity when an explicit path addresses exactly that identity.
    pub fn addressable(&self, id: PluginId) -> Option<&PluginName> {
        self.directories
            .get(&id)
            .filter(|directory| self.addresses.get(*directory) == Some(&id))
    }

    /// Directories that several of these identities would be written to.
    pub fn collisions(&self, ids: impl IntoIterator<Item = PluginId>) -> BTreeSet<PluginName> {
        let mut seen = BTreeSet::new();
        ids.into_iter()
            .filter_map(|id| self.directories.get(&id))
            .filter(|directory| !seen.insert(*directory))
            .cloned()
            .collect()
    }

    fn checked(&self, selection: RewindSelection) -> Result<RewindSelection, ScopeError> {
        match self
            .collisions(selection.plugins.keys().copied())
            .into_iter()
            .next()
        {
            Some(directory) => Err(ScopeError::Collision(directory)),
            None => Ok(selection),
        }
    }
}

/// An accepted name, plus the code choice and current host implementation available to it.
#[derive(Clone, serde::Serialize)]
pub struct RegisteredPlugin {
    /// Immutable plugin identity.
    pub id: PluginId,
    /// Registration that supplied this name; absent for a newly supplied factory name.
    pub named_by: Option<RegistrationId>,
    /// Last accepted code intent; absent for a factory name that has never registered one.
    pub code: Option<RegisteredSource>,
    /// Generation selected by the latest accepted activation, or this host's unregistered factory code.
    pub activation: Activation,
    /// Host-owned code and its follow decision, when the name is reserved.
    pub factory: Option<FactoryStatus>,
}

/// One selected generation and where it came from; the running and registered layers share it.
#[derive(Clone, serde::Serialize)]
pub struct Activation {
    /// Selected generation and its current outcome.
    pub generation: GenerationRecord,
    /// Registration that selected it; absent for the host's factory default.
    pub registration: Option<RegistrationId>,
    /// Where the generation's code came from.
    pub code: CodeProvenance,
}

/// Where a generation's code came from; settings-only generations follow their code generation.
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct CodeProvenance {
    /// Generation that supplied the code.
    pub generation: GenerationId,
    /// How that code entered the registry.
    pub source: CodeSource,
}

/// How a generation's code entered the registry.
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeSource {
    /// Code shipped with this host, registered or not.
    Factory,
    /// A registered source directory.
    Directory {
        /// Registration that registered the directory.
        registration: RegistrationId,
        /// The directory's content, as stored when it was registered.
        source: ContentHash,
    },
}

impl RegisteredPlugin {
    /// Address of the accepted source directory, absent when following factory code.
    pub fn source(&self) -> Option<ContentHash> {
        self.code.as_ref()?.source()
    }
}

/// An explicitly registered code choice, independent of mechanical recovery.
#[derive(Clone, serde::Serialize)]
pub struct RegisteredSource {
    /// Directory or host factory code.
    pub intent: CodeIntent,
    /// Accepted activation, including reuse of an existing factory generation.
    pub registration: RegistrationId,
    /// Generation selected by that activation.
    pub generation: GenerationId,
    /// Artifact used to compare newly built code.
    pub artifact: ContentHash,
}

impl RegisteredSource {
    /// Address of the selected source directory, absent for factory code.
    pub fn source(&self) -> Option<ContentHash> {
        match self.intent {
            CodeIntent::Directory { source } => Some(source),
            CodeIntent::Factory => None,
        }
    }

    /// A source build matches only a registered directory choice, even when factory bytes coincide.
    pub fn matches_source_artifact(&self, artifact: ContentHash) -> bool {
        matches!(self.intent, CodeIntent::Directory { .. }) && self.artifact == artifact
    }
}

/// Current host-owned recovery code and whether registered intent needs to follow it.
#[derive(Clone, serde::Serialize)]
pub struct FactoryStatus {
    /// Name reserved by the host.
    pub name: PluginName,
    /// Recorded embedded code.
    pub generation: GenerationId,
    /// Content address of the embedded code.
    pub artifact: ContentHash,
    /// Registered intent follows factory code and differs from this host's implementation.
    pub follow: bool,
}

impl Registry {
    /// Read accepted intent without assembling runtime status or generation history.
    pub async fn registered_intent(
        &self,
        at: Option<RegistrationId>,
    ) -> Result<RegisteredIntent, RegistryError> {
        let state = self.state.lock().await;
        if let Some(id) = at
            && !state.history.registrations.contains_key(&id)
        {
            return Err(state.unknown_registration(id));
        }
        state.registered_intent(&self.deps.factory, at)
    }
}

impl State {
    pub fn registered_intent(
        &self,
        factories: &[FactoryPlugin],
        at: Option<RegistrationId>,
    ) -> Result<RegisteredIntent, RegistryError> {
        let plugins = self
            .registered_identities_at(factories, at)
            .into_iter()
            .map(|(name, id)| {
                let code = self.registered_source(id, at);
                let factory = factories
                    .iter()
                    .find(|factory| factory.name == name)
                    .map(|factory| self.factory_status(factory, code.as_ref()));
                let reference = self
                    .registrations_through(at)
                    .rev()
                    .find_map(|(_, registration)| {
                        registration.code.get(&id).map(|code| PluginActivation {
                            generation: code.generation,
                            registration: Some(registration.id),
                        })
                    })
                    .or_else(|| {
                        factory.as_ref().map(|factory| PluginActivation {
                            generation: factory.generation,
                            registration: None,
                        })
                    })
                    .ok_or_else(|| {
                        RegistryError::Intent(format!("registered plugin {name} has no code"))
                    })?;
                Ok((
                    name,
                    RegisteredPlugin {
                        id,
                        named_by: self
                            .registered_name(id, at)
                            .map(|(registration, _)| registration),
                        code,
                        factory,
                        activation: self.activation(reference)?,
                    },
                ))
            })
            .collect::<Result<_, RegistryError>>()?;
        let base = self
            .registrations_through(at)
            .next_back()
            .map(|(id, _)| -> Result<RegistrationBase, RegistryError> {
                let configuration = self
                    .registered_config_at(at)
                    .ok_or_else(|| {
                        RegistryError::Intent(format!("registration {id} has no configuration"))
                    })?
                    .clone();
                Ok(RegistrationBase {
                    id: *id,
                    configuration,
                })
            })
            .transpose()?;
        Ok(RegisteredIntent { base, plugins })
    }

    /// Accepted names plus host-reserved names: the one mapping intent changes use for identity.
    pub fn registered_identities(
        &self,
        factories: &[FactoryPlugin],
    ) -> BTreeMap<PluginName, PluginId> {
        self.registered_identities_at(factories, None)
    }

    fn registered_identities_at(
        &self,
        factories: &[FactoryPlugin],
        at: Option<RegistrationId>,
    ) -> BTreeMap<PluginName, PluginId> {
        let mut identities = self.registered_names_at(at);
        for factory in factories {
            identities.entry(factory.name.clone()).or_insert(factory.id);
        }
        identities
    }

    fn factory_status(
        &self,
        factory: &FactoryPlugin,
        code: Option<&RegisteredSource>,
    ) -> FactoryStatus {
        let generation = self.factory[&factory.id];
        let artifact = self.history.generations[&generation].artifact;
        FactoryStatus {
            name: factory.name.clone(),
            generation,
            artifact,
            follow: code
                .is_none_or(|code| code.intent == CodeIntent::Factory && code.artifact != artifact),
        }
    }

    fn registered_source(
        &self,
        plugin: PluginId,
        at: Option<RegistrationId>,
    ) -> Option<RegisteredSource> {
        self.registrations_through(at)
            .rev()
            .find_map(|(_, registration)| {
                let code = registration.code.get(&plugin)?;
                Some(RegisteredSource {
                    intent: code.intent?,
                    registration: registration.id,
                    generation: code.generation,
                    artifact: self.history.generations[&code.generation].artifact,
                })
            })
    }
}
