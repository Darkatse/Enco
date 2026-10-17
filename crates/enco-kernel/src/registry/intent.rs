//! The registered intent used by management and inspection, independent of running references.
use super::*;

/// The accepted registration against which a new change is prepared.
#[derive(Clone, serde::Serialize)]
pub struct RegistrationBase {
    /// Structural registration identity.
    pub id: RegistrationId,
    /// Commit containing the complete accepted file intent.
    pub revision: String,
}

/// Only the facts needed to compare and prepare new intent.
#[derive(Clone, serde::Serialize)]
pub struct RegisteredIntent {
    /// Absent only while preparing the first registration.
    pub base: Option<RegistrationBase>,
    /// Latest accepted configuration, absent before first registration.
    pub configuration: Option<ConfigGeneration>,
    /// Accepted and host-reserved names with their immutable identities.
    pub plugins: BTreeMap<String, RegisteredPlugin>,
}

/// An accepted name, plus the code choice and current host implementation available to it.
#[derive(Clone, serde::Serialize)]
pub struct RegisteredPlugin {
    /// Immutable installation identity.
    pub id: PluginId,
    /// Last accepted code intent; direct artifacts do not introduce source intent.
    pub code: Option<RegisteredSource>,
    /// Host-owned code and its follow decision, when the name is reserved.
    pub factory: Option<FactoryStatus>,
}

/// An explicitly registered code choice, independent of mechanical recovery.
#[derive(Clone, serde::Serialize)]
pub struct RegisteredSource {
    /// Directory or host factory code.
    pub kind: CodeIntent,
    /// Accepted activation, including reuse of an existing factory generation.
    pub registration: RegistrationId,
    /// Generation selected by that activation.
    pub generation: GenerationId,
    /// Artifact used to compare newly built code.
    pub artifact: ContentHash,
}

/// Current host-owned recovery code and whether registered intent needs to follow it.
#[derive(Clone, serde::Serialize)]
pub struct FactoryStatus {
    /// Name reserved by the host.
    pub name: String,
    /// Recorded embedded code.
    pub generation: GenerationId,
    /// Content address of the embedded code.
    pub artifact: ContentHash,
    /// Registered intent follows factory code and differs from this host's implementation.
    pub follow: bool,
}

impl Registry {
    /// Read accepted intent without assembling runtime status or generation history.
    pub async fn registered_intent(&self) -> RegisteredIntent {
        self.state
            .lock()
            .await
            .registered_intent(&self.deps.factory)
    }
}

impl State {
    pub fn registered_intent(&self, factories: &[FactoryPlugin]) -> RegisteredIntent {
        let plugins = self
            .registered_identities(factories)
            .into_iter()
            .map(|(name, id)| {
                let code = self.registered_source(id);
                let factory = factories
                    .iter()
                    .find(|factory| factory.name == name)
                    .map(|factory| self.factory_status(factory, code.as_ref()));
                (name, RegisteredPlugin { id, code, factory })
            })
            .collect();
        RegisteredIntent {
            base: self
                .history
                .registrations
                .values()
                .next_back()
                .map(|record| RegistrationBase {
                    id: record.id,
                    revision: record.revision.clone(),
                }),
            configuration: self.registered_config().cloned(),
            plugins,
        }
    }

    /// Accepted names plus host-reserved names: the one mapping intent changes use for identity.
    pub fn registered_identities(&self, factories: &[FactoryPlugin]) -> BTreeMap<String, PluginId> {
        let mut identities = self.registered_names();
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
                .is_none_or(|code| code.kind == CodeIntent::Factory && code.artifact != artifact),
        }
    }

    fn registered_source(&self, plugin: PluginId) -> Option<RegisteredSource> {
        self.history
            .registrations
            .values()
            .rev()
            .find_map(|registration| {
                let code = registration.code.get(&plugin)?;
                Some(RegisteredSource {
                    kind: code.intent?,
                    registration: registration.id,
                    generation: code.generation,
                    artifact: self.history.generations[&code.generation].artifact,
                })
            })
    }
}
