//! The single committer of plugin identities, generation history and published exports.
mod activation;
mod admission;
mod configuration;
mod exports;
mod health;
mod probe;
mod registration;
mod state;
mod status;
mod wiring;

use crate::{
    Clock, Credential, Decision, Embedding, Endpoint, KernelSettings, Loaded, NewGeneration,
    Provider, Runtime, SettingsError, Store, StoreError,
};
use activation::ActivationReason;
use admission::{users, validate_name};
use enco_core::*;
pub use exports::{Export, Exports};
use state::State;
pub use status::{ConfigStatus, PluginStatus, RegistryStatus};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, watch};

/// Durable registry dependencies assembled by the application.
pub struct RegistryDeps {
    /// Sole persistence port, including plugins.lock and immutable artifacts.
    pub store: Arc<dyn Store>,
    /// Component compilation and export discovery.
    pub runtime: Arc<dyn Runtime>,
    /// Host-reserved identities and embedded bytes.
    pub factory: Vec<FactoryPlugin>,
    /// Host-owned tables and their declaration functions.
    pub owners: Vec<TableOwner>,
    /// Initial intent, used only when no configuration generation exists.
    pub bootstrap: Result<serde_json::Value, String>,
    /// Names from the native tool declarations assembled by the composition root.
    pub host_tool_names: Vec<String>,
    /// Observation time, never generation ordering.
    pub clock: Arc<dyn Clock>,
}

/// A plugin supplied with the host binary.
pub struct FactoryPlugin {
    /// Reserved name in plugins.lock.
    pub name: String,
    /// Stable host-assigned identity, independent of component self-description.
    pub id: PluginId,
    /// Embedded component bytes.
    pub artifact: Vec<u8>,
}

/// A host-owned group of tables and its pure declaration function.
pub struct TableOwner {
    /// Top-level tables exclusively owned by this component.
    pub tables: &'static [&'static str],
    /// Parse the owned tables and project their endpoint requirements.
    pub declare: fn(&serde_json::Value, &KernelSettings) -> Result<Vec<Use>, SettingsError>,
}

/// One configured user's endpoint and requirements.
#[derive(Clone)]
pub struct Use {
    /// Human-readable user, such as `profile default.reply`.
    pub user: String,
    /// Resolved service and invocation parameters.
    pub endpoint: Endpoint,
    /// Required interface and behavior.
    pub need: Need,
}

/// A use's interface and behavior requirements, shared by wiring checks and real probes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A completion endpoint, optionally with tool definitions.
    Completion {
        /// Whether normal requests include tools.
        tools: bool,
    },
    /// Embeddings of the configured width.
    Embedding {
        /// Expected vector width.
        dimensions: std::num::NonZeroUsize,
    },
    /// Typed judgments for host policies.
    Decision,
}

/// Plugin content to register, including when undoing an earlier change.
pub enum PluginSource {
    /// New component bytes; inherit the active generation's complete settings.
    Artifact(Vec<u8>),
    /// All content of an existing plugin generation.
    Generation(GenerationId),
}

/// Configuration content to register.
pub enum ConfigSource {
    /// A complete document from the intent file.
    Document(serde_json::Value),
    /// The document of an existing configuration generation.
    Generation(GenerationId),
}

/// The one published snapshot of routing and registered configuration.
pub struct View {
    /// Derived invocation routes.
    pub exports: Exports,
    /// Registered content and parsed kernel tables.
    pub config: ActiveConfig,
}

/// The active configuration generation: registered content plus the declarations the current
/// host derives from it. The declarations are private to the registry, so only the registry
/// builds one, from a single evaluation, and the parts cannot drift apart.
#[derive(Clone)]
pub struct ActiveConfig {
    /// Active configuration generation.
    pub generation: GenerationId,
    /// Registered document; never read from the intent file during use.
    pub document: serde_json::Value,
    /// Parsed kernel-owned tables.
    pub kernel: KernelSettings,
    /// Endpoint users declared by the kernel and the host owners.
    uses: Vec<Use>,
    /// Tables the current host cannot interpret; empty after any intentional registration.
    invalid: Vec<SettingsError>,
    /// Plugin settings requested by this configuration; active plugin content may have rolled back.
    plugins: BTreeMap<String, PluginSettings>,
}

impl View {
    /// The registered behavior a failure is recorded against: the active configuration and
    /// plugin generations, in ascending order. Trial promotion leaves them unchanged.
    pub(crate) fn active_generations(&self) -> Vec<GenerationId> {
        let mut generations: Vec<_> = self
            .exports
            .active_generations()
            .chain([self.config.generation])
            .collect();
        generations.sort_unstable();
        generations
    }
}

/// Routing is published only after the durable mutation commits.
pub struct Registry {
    store: Arc<dyn Store>,
    runtime: Arc<dyn Runtime>,
    clock: Arc<dyn Clock>,
    owners: Vec<TableOwner>,
    pub(crate) host_tool_names: Vec<String>,
    state: Mutex<State>,
    views: watch::Sender<Arc<View>>,
}

/// One completed invocation's verdict, before caller-specific result handling.
pub enum Verdict {
    /// A valid plugin result, regardless of the caller's subsequent policy checks.
    Ok,
    /// A failed invocation; only attributable plugin faults affect health.
    Failed(Failure),
}

/// Result of accepting a component as a new generation.
#[derive(serde::Serialize)]
pub struct Deployed {
    /// Newly committed generation.
    pub generation: GenerationRecord,
    /// Configured callers affected by the change.
    pub users: Vec<String>,
}

/// Result of applying configuration content.
pub enum Applied {
    /// Newly registered and active content.
    Registered {
        /// New global generation number.
        generation: GenerationId,
        /// Previously active configuration.
        previous: GenerationId,
        /// Real calls made for newly declared uses.
        probes: Vec<Probe>,
    },
    /// The content was already active; no calls or writes occurred.
    Unchanged(GenerationId),
}

/// A successful real call made while applying configuration.
#[derive(serde::Serialize)]
pub struct Probe {
    /// User whose requirement triggered this call.
    pub user: String,
    /// Endpoint that was checked.
    pub endpoint: String,
}

/// How the current intent file relates to registered configuration content.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigDivergence {
    /// Intent equals the active configuration.
    None,
    /// The document is registered, but these active plugin generations use different settings.
    PluginSettings {
        /// Current plugin generations, each differing from its section in the active document.
        active: BTreeMap<String, GenerationId>,
    },
    /// Intent has never been registered, or cannot be read.
    Unapplied {
        /// Read or parse error, when present.
        reason: Option<String>,
    },
    /// Intent was registered but a different generation is active.
    Superseded {
        /// Latest generation matching the intent document.
        generation: GenerationId,
        /// Explanation derived from the append-only generation history.
        reason: String,
    },
}

/// An explicit registry operation could not complete.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A reserved identity in plugins.lock differs from the host's identity.
    #[error("plugins.lock records a different identity for factory plugin {0}")]
    FactoryIdentity(String),
    /// No registered plugin matches this name or identity.
    #[error("unknown plugin {0}")]
    UnknownPlugin(String),
    /// The component, name or its exports cannot satisfy the operation.
    #[error("{name} rejected: {reason}")]
    Rejected {
        /// Name supplied by the caller.
        name: String,
        /// Loading or admission failure.
        reason: String,
    },
    /// The requested number is absent from both histories.
    #[error("unknown generation {0}")]
    UnknownGeneration(GenerationId),
    /// The active configuration does not define this profile.
    #[error("unknown profile {0}")]
    UnknownProfile(String),
    /// Registered kernel settings cannot be interpreted by this host.
    #[error("configuration: {0}")]
    Config(SettingsError),
    /// No initial document is available for the first configuration registration.
    #[error("initial configuration unavailable: {0}")]
    Bootstrap(String),
    /// The host composition assigns a table to more than one owner.
    #[error("table {0} has multiple owners")]
    TableOwner(String),
    /// No usable recovery generation exists.
    #[error("{0} has no generation to roll back to")]
    NoRollbackTarget(String),
    /// The active generation or target health changed while the target was prepared.
    #[error("{0}: the registry changed while preparing; retry")]
    Conflict(String),
}

impl Registry {
    /// Restore persisted routing, register embedded artifacts and validate configured uses.
    pub async fn open(deps: RegistryDeps) -> Result<Arc<Self>, RegistryError> {
        configuration::check_owners(&deps.owners)?;
        let (state, admission) = State::initialize(&deps).await?;
        let (views, _) = watch::channel(Arc::new(state.view(admission)));
        Ok(Arc::new(Self {
            store: deps.store,
            runtime: deps.runtime,
            clock: deps.clock,
            owners: deps.owners,
            host_tool_names: deps.host_tool_names,
            state: Mutex::new(state),
            views,
        }))
    }

    /// Read one immutable snapshot, immediately releasing the watch borrow.
    pub fn view(&self) -> Arc<View> {
        self.views.borrow().clone()
    }

    /// Follow published views at a component's own boundary.
    pub fn subscribe(&self) -> watch::Receiver<Arc<View>> {
        self.views.subscribe()
    }

    /// Load outside the commit lock, then admit and activate against the latest state.
    pub async fn deploy(
        &self,
        name: &str,
        source: PluginSource,
    ) -> Result<Deployed, RegistryError> {
        validate_name(name)?;
        let state = self.state.lock().await;
        let expected_plugin = state
            .plugins
            .names
            .get(name)
            .and_then(|id| state.plugins.active.get(id))
            .copied();
        let expected_config = state.config.generation;
        let (artifact, hash, settings) = match source {
            PluginSource::Artifact(bytes) => {
                let settings = expected_plugin
                    .and_then(|id| state.plugins.generations.get(&id))
                    .map_or_else(PluginSettings::default, |record| record.settings.clone());
                drop(state);
                let hash = self.store.put_artifact(&bytes).await?;
                (bytes, hash, settings)
            }
            PluginSource::Generation(id) => {
                let record = state.plugins.generations.get(&id).ok_or_else(|| {
                    if state.configs.contains_key(&id) {
                        rejected(name, "generation is configuration content")
                    } else {
                        RegistryError::UnknownGeneration(id)
                    }
                })?;
                if state.plugins.names.get(name) != Some(&record.plugin) {
                    return Err(rejected(name, "generation belongs to another plugin"));
                }
                let record = record.clone();
                drop(state);
                (
                    self.store.artifact(&record.artifact).await?,
                    record.artifact,
                    record.settings,
                )
            }
        };
        let loaded = registration::load(self.runtime.as_ref(), name, &artifact, &settings).await?;
        let mut state = self.state.lock().await;
        let current = state
            .plugins
            .names
            .get(name)
            .and_then(|id| state.plugins.active.get(id))
            .copied();
        if current != expected_plugin || state.config.generation != expected_config {
            return Err(RegistryError::Conflict(name.into()));
        }
        let candidates = BTreeMap::from([(name.to_owned(), loaded.clone())]);
        let before = admission::evaluate(&state.plugins, &self.host_tool_names, &BTreeMap::new());
        let admission = admission::evaluate(&state.plugins, &self.host_tool_names, &candidates);
        admission.accept_change(
            &before,
            &[name.into()],
            &state.config.uses,
            &state.config.uses,
        )?;
        let plugin = match state.plugins.names.get(name) {
            Some(id) => *id,
            None => {
                let id = PluginId::new();
                self.store.register_plugin(name, id).await?;
                state.plugins.names.insert(name.into(), id);
                id
            }
        };
        let generation = NewGeneration {
            plugin,
            artifact: hash,
            settings,
            origin: Origin::Deployed,
            status: GenerationStatus::Trial,
            created_at: self.clock.now(),
        };
        let id = self.store.insert_generation(&generation, true).await?;
        let record = generation.numbered(id);
        state.plugins.registered(record.clone(), loaded);
        self.publish(&mut state, Some(admission));
        Ok(Deployed {
            generation: record,
            users: users(&state.config.uses, name),
        })
    }

    /// Roll back relative to the generation observed when choosing the target.
    pub async fn rollback(&self, name: &str) -> Result<GenerationRecord, RegistryError> {
        let (plugin, expected, candidates) = {
            let state = self.state.lock().await;
            let plugin = *state
                .plugins
                .names
                .get(name)
                .ok_or_else(|| RegistryError::UnknownPlugin(name.into()))?;
            let expected = state
                .plugins
                .active
                .get(&plugin)
                .copied()
                .ok_or_else(|| RegistryError::NoRollbackTarget(name.into()))?;
            (plugin, expected, state.plugins.candidates(plugin, expected))
        };
        if candidates.is_empty() {
            return Err(RegistryError::NoRollbackTarget(name.into()));
        }
        self.activate(name, plugin, expected, candidates, ActivationReason::Manual)
            .await?
            .target
            .ok_or_else(|| RegistryError::NoRollbackTarget(name.into()))
    }

    fn publish(&self, state: &mut State, admission: Option<admission::Admission>) {
        state.plugins.loaded.retain(|id, _| {
            state.plugins.active.values().any(|active| active == id)
                || state.plugins.factory.values().any(|factory| factory == id)
        });
        state.plugins.trial_successes.retain(|id, _| {
            state.plugins.active.values().any(|active| active == id)
                && state
                    .plugins
                    .generations
                    .get(id)
                    .is_some_and(|record| record.status == GenerationStatus::Trial)
        });
        let admission = admission.unwrap_or_else(|| {
            admission::evaluate(&state.plugins, &self.host_tool_names, &BTreeMap::new())
        });
        self.views.send_replace(Arc::new(state.view(admission)));
    }
}

fn rejected(name: &str, reason: impl ToString) -> RegistryError {
    RegistryError::Rejected {
        name: name.into(),
        reason: reason.to_string(),
    }
}
