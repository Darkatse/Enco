//! The single committer of plugin identities, generation history and published exports.
mod activation;
mod admission;
mod configuration;
mod exports;
mod factory;
mod health;
mod intent;
mod probe;
mod registration;
mod request;
mod startup;
mod state;
mod status;
mod wiring;

use crate::{
    Clock, Credential, Decision, Embedding, Endpoint, KernelSettings, Loaded, Provider, Runtime,
    SettingsError, Store, StoreError,
};
use admission::{users, validate_name};
use enco_core::*;
pub use exports::{Export, Exports};
pub use intent::{
    FactoryStatus, RegisteredIntent, RegisteredPlugin, RegisteredSource, RegistrationBase,
};
pub use request::{ArtifactSource, PluginChange, Registered, RegistrationRequest, Rollback};
use state::State;
pub use status::{ConfigStatus, PluginStatus, RegistryStatus};
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
    /// Host-owned tables and their declaration functions.
    pub owners: Vec<TableOwner>,
    /// Names from the native tool declarations assembled by the composition root.
    pub host_tool_names: Vec<String>,
    /// Observation time, never generation ordering.
    pub clock: Arc<dyn Clock>,
}

/// A plugin supplied with the host binary.
#[derive(Clone)]
pub struct FactoryPlugin {
    /// Host-reserved routing name.
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
    deps: RegistryDeps,
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
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A stored factory identity differs from the host's reserved identity.
    #[error("registry records a different identity for factory plugin {0}")]
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
    /// Restore registered behavior; only an empty registry consumes initial intent.
    pub async fn open<E, F>(
        deps: RegistryDeps,
        bootstrap: impl FnOnce(RegisteredIntent) -> F,
    ) -> Result<Arc<Self>, E>
    where
        E: From<RegistryError>,
        F: std::future::Future<Output = Result<RegistrationRequest, E>>,
    {
        configuration::check_owners(&deps.owners)?;
        let (state, config) = startup::open(&deps, bootstrap).await?;
        let admission = admission::evaluate(state.contents(), &deps.host_tool_names);
        let (views, _) = watch::channel(Arc::new(state.view(config, admission, &deps.factory)));
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
        self.views
            .send_replace(Arc::new(state.view(config, admission, &self.deps.factory)));
    }
}

/// Validate the directory and routing name accepted by every plugin entry point.
pub fn validate_plugin_name(name: &str) -> Result<(), RegistryError> {
    validate_name(name)
}

fn rejected(name: &str, reason: impl ToString) -> RegistryError {
    RegistryError::Rejected {
        name: name.into(),
        reason: reason.to_string(),
    }
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
