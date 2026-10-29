//! The single committer of plugin identities, generation history and published exports.
mod activation;
mod admission;
pub use admission::ToolOwner;
mod cause;
pub use cause::{Cause, IntentPart, Proposed, Referrer};
mod configuration;
mod exports;
mod factory;
mod health;
mod history;
pub use history::{PluginChangeKind, RegistrationDetails, SelectedGeneration, UnitChange};
mod intent;
mod probe;
mod registration;
mod request;
mod startup;
mod state;
mod status;
mod wiring;

use crate::{
    Channel, ChannelSettings, Clock, Composer, ComposerOptions, ComposerSettings, Credential,
    Decision, Embedding, Endpoint, KernelSettings, LoadError, Loaded, Provider, Runtime,
    SettingsError, Store, StoreError,
};
use admission::users;
pub use configuration::{PLUGIN_SETTINGS_TABLE, plugin_settings};
use enco_core::*;
pub use exports::{Active, BoundComposer, Export, Exports};
pub use intent::{
    Activation, CodeProvenance, CodeSource, FactoryStatus, RegisteredIntent, RegisteredPlugin,
    RegisteredSource, RegistrationBase, RewindScope, RewindSelection, ScopeError,
};
pub use request::{
    ArtifactSource, ConfigurationSource, PluginChange, Registered, RegistrationRequest, Rollback,
};
use state::State;
pub use status::{
    ConfigStatus, Difference, PluginStatus, RECENT_REGISTRATIONS, RegistryStatus, Running,
    Standing, UnservedUse,
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, watch};

/// Durable registry dependencies assembled by the application.
pub struct RegistryDeps {
    /// Sole persistence port for registry facts and component artifacts.
    pub store: Arc<dyn Store>,
    /// Component compilation and export discovery.
    pub runtime: Arc<dyn Runtime>,
    /// Host-reserved identities and embedded bytes.
    pub factory: Vec<FactoryPlugin>,
    /// The factory plugin that composes for profiles naming no composer, and in safe mode.
    pub composer: PluginName,
    /// Host-owned tables and their declaration functions.
    pub owners: Vec<TableOwner>,
    /// Names from the native tool declarations assembled by the composition root.
    pub host_tool_names: Vec<String>,
    /// Native repair tool names required by profiles that request the lifeline.
    pub lifeline: Vec<String>,
    /// Observation time, never generation ordering.
    pub clock: Arc<dyn Clock>,
}

/// A plugin supplied with the host binary.
#[derive(Clone)]
pub struct FactoryPlugin {
    /// Host-reserved routing name.
    pub name: PluginName,
    /// Stable host-assigned identity, independent of component self-description.
    pub id: PluginId,
    /// Embedded component bytes.
    pub artifact: Vec<u8>,
}

/// A host-owned group of tables and its pure declaration function.
pub struct TableOwner {
    /// Top-level tables exclusively owned by this component.
    pub tables: &'static [ConfigTable],
    /// Parse the owned tables and project their configured invocations.
    pub declare: fn(&serde_json::Value, &KernelSettings) -> Result<Vec<Use>, SettingsError>,
}

/// The structural unit owned by one configuration declaration.
#[derive(Clone, Copy)]
pub enum ConfigTable {
    /// The table is one configuration section.
    Single(&'static str),
    /// Each named entry is a configuration section.
    Named(&'static str),
}

impl ConfigTable {
    /// Top-level key owned by the declaration.
    pub fn name(self) -> &'static str {
        match self {
            Self::Single(name) | Self::Named(name) => name,
        }
    }
}

/// One configured caller and the invocation it requires.
#[derive(Clone)]
pub struct Use {
    /// Human-readable caller, such as `profile default.reply`.
    pub user: String,
    /// The invocation this caller configures.
    pub target: UseTarget,
}

/// The configured invocation, preserving the facts its actual interface consumes.
#[derive(Clone)]
pub enum UseTarget {
    /// One connection's pure channel mapper and credential bindings.
    Channel {
        /// Connection name in the channel table.
        name: String,
        /// Registered plugin exporting the protocol mapper.
        plugin: PluginName,
        /// Transport parameters and credential names.
        settings: ChannelSettings,
    },
    /// A model service and the behavior its consumer requires.
    Endpoint {
        /// Named service and its invocation parameters.
        endpoint: Endpoint,
        /// Required model interface and behavior.
        need: Need,
    },
    /// A profile's pure composer invocation.
    Composer {
        /// Profile owning these options, used to attribute repair.
        profile: String,
        /// Plugin and invocation options.
        settings: ComposerSettings,
    },
}

/// The configuration entry a use names, without copying its invocation parameters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UseRef {
    /// A named channel connection.
    Channel {
        /// Name in the channel table.
        name: String,
    },
    /// A named model service.
    Endpoint {
        /// Name in the endpoint table.
        endpoint: String,
    },
    /// A profile's composer selection and options.
    Composer {
        /// Name in the profile table.
        profile: String,
    },
}

impl std::fmt::Display for UseRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Channel { name } => write!(f, "channel {name}"),
            Self::Endpoint { endpoint } => write!(f, "endpoint {endpoint}"),
            Self::Composer { .. } => f.write_str("composer"),
        }
    }
}

/// A use's interface and behavior requirements, shared by wiring checks and real probes.
#[derive(Clone, PartialEq, Eq)]
pub enum Need {
    /// A completion endpoint, optionally with tool definitions.
    Completion {
        /// Whether normal requests include tools.
        tools: bool,
        /// Output budget that normal requests declare.
        max_output_tokens: std::num::NonZeroU32,
        /// Non-text inputs used by these completion calls.
        accepts: std::collections::BTreeSet<Modality>,
    },
    /// Embeddings of the configured width.
    Embedding {
        /// Expected vector width.
        dimensions: std::num::NonZeroUsize,
    },
    /// Typed judgments for host policies.
    Decision,
}

/// The one published snapshot of routing and registered configuration.
pub struct View {
    /// Derived invocation routes.
    pub exports: Exports,
    /// Registered content and parsed kernel tables.
    pub config: ActiveConfig,
}

/// The active configuration generation: registered content plus the declarations derived from it.
///
/// The declarations are those the current host derives. They are private to the registry, so only
/// the registry builds one, from a single evaluation, and the parts cannot drift apart.
#[derive(Clone)]
pub struct ActiveConfig {
    /// Active configuration generation.
    pub generation: GenerationId,
    /// Registered document; never read from the intent file during use.
    pub document: serde_json::Value,
    /// Parsed kernel-owned tables.
    pub kernel: KernelSettings,
    /// Configured uses declared by the kernel and the host owners.
    uses: Vec<Use>,
    /// Tables the current host cannot interpret; empty after any intentional registration.
    invalid: Vec<SettingsError>,
    /// Plugin settings requested by this configuration; active plugin content may have rolled back.
    plugins: BTreeMap<PluginName, PluginSettings>,
}

impl View {
    /// The registered behavior a failure is recorded against: the active configuration and
    /// plugin generations, in ascending order.
    ///
    /// Trial promotion leaves them unchanged.
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
    deps: RegistryDeps,
    state: Mutex<State>,
    views: watch::Sender<Arc<View>>,
}

/// One completed invocation's verdict, before caller-specific result handling.
pub enum Verdict {
    /// The result passed its interface contract checks, including composer plan binding.
    Ok,
    /// A failed invocation; only attributable plugin faults affect health.
    Failed(Failure),
}

/// A successful real call made while applying configuration.
#[derive(serde::Serialize)]
pub struct Probe {
    /// User whose requirement triggered this call.
    pub user: String,
    /// Endpoint that was checked.
    pub endpoint: String,
}

/// An explicit registry operation could not complete.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// No registration exists yet; only bootstrap preparation may use an empty baseline.
    #[error("no registration has been accepted")]
    NoRegistrations,
    /// Recorded facts cannot form a complete accepted view.
    #[error("registered facts are inconsistent: {0}")]
    Intent(String),
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A stored factory identity differs from the host's reserved identity.
    #[error("registry records a different identity for factory plugin {0}")]
    FactoryIdentity(PluginName),
    /// No registered plugin matches this name.
    #[error("unknown plugin {0}")]
    UnknownPlugin(PluginName),
    /// The intent is rejected; each cause names the part it concerns.
    #[error("rejected: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Rejected(Vec<Cause>),
    /// A registration request supplied no change.
    #[error("registration request has no changes")]
    EmptyRequest,
    /// Factory intent was requested for a name the host does not reserve.
    #[error("{0} is not a factory plugin")]
    NotFactory(PluginName),
    /// Removal was requested for a host-reserved name; factory intent replaces source instead.
    #[error("factory plugin {0} cannot be removed; register factory intent")]
    FactoryRemoval(PluginName),
    /// The requested number is absent from both histories.
    #[error("unknown generation {0}")]
    UnknownGeneration(GenerationId),
    /// No accepted change has this structural position.
    #[error("unknown registration {registration} (latest: {latest})")]
    UnknownRegistration {
        /// Requested position.
        registration: RegistrationId,
        /// Latest accepted position.
        latest: RegistrationId,
    },
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
    /// No accepted change remains to roll back.
    #[error("no registration can be rolled back")]
    NoRollbackTarget,
    /// The active generation or target health changed while the target was prepared.
    #[error("{0}: the registry changed while preparing; retry")]
    Conflict(String),
}

impl Registry {
    /// Restore registered behavior; only an empty registry consumes initial intent.
    pub async fn open<E, F>(
        deps: RegistryDeps,
        bootstrap: impl FnOnce(RegisteredIntent) -> F,
        registry_error: impl Fn(RegistryError) -> E,
    ) -> Result<Arc<Self>, E>
    where
        F: std::future::Future<Output = Result<RegistrationRequest, E>>,
    {
        configuration::check_owners(&deps.owners).map_err(&registry_error)?;
        let (state, config) = startup::open(&deps, bootstrap, &registry_error).await?;
        let admission = admission::evaluate(state.contents(), &deps.host_tool_names);
        let (views, _) = watch::channel(Arc::new(state.view(
            config,
            admission,
            &deps.factory,
            &deps.composer,
        )));
        Ok(Arc::new(Self {
            deps,
            state: Mutex::new(state),
            views,
        }))
    }

    /// Read an immutable view without retaining a watch borrow.
    pub fn view(&self) -> Arc<View> {
        self.views.borrow().clone()
    }

    pub(crate) fn host_tool_names(&self) -> &[String] {
        &self.deps.host_tool_names
    }

    /// Each component follows changes at its own ownership boundary.
    pub fn subscribe(&self) -> watch::Receiver<Arc<View>> {
        self.views.subscribe()
    }

    fn publish(
        &self,
        state: &mut State,
        config: Option<ActiveConfig>,
        admission: Option<admission::Admission>,
    ) {
        state.release();
        let config = config.unwrap_or_else(|| self.view().config.clone());
        let admission = admission
            .unwrap_or_else(|| admission::evaluate(state.contents(), &self.deps.host_tool_names));
        self.views.send_replace(Arc::new(state.view(
            config,
            admission,
            &self.deps.factory,
            &self.deps.composer,
        )));
    }
}

fn rejected(cause: Cause) -> RegistryError {
    RegistryError::Rejected(vec![cause])
}

/// Failed durable writes leave the previous references authoritative for artifact retention.
async fn cleanup_failed_write(
    deps: &RegistryDeps,
    state: &State,
    error: RegistryError,
) -> RegistryError {
    match deps
        .store
        .retain_artifacts(&state.retained_artifacts())
        .await
    {
        Ok(()) => error,
        Err(cleanup) => StoreError::Backend(format!(
            "{error}; releasing unaccepted artifacts also failed: {cleanup}"
        ))
        .into(),
    }
}

/// Cleanup follows committed references; a failure cannot undo their acceptance.
async fn cleanup_artifacts(deps: &RegistryDeps, state: &State) -> Option<String> {
    match deps
        .store
        .retain_artifacts(&state.retained_artifacts())
        .await
    {
        Ok(()) => None,
        Err(error) => {
            let warning = format!("artifact cleanup: {error}");
            tracing::warn!("{warning}");
            Some(warning)
        }
    }
}
