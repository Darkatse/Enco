use crate::{CallContext, Channel, Composer, Decision, Embedding, Provider};
use async_trait::async_trait;
use enco_core::{Failure, Outcome, PluginSettings, ToolSpec};
use std::sync::Arc;

/// Compile and describe component bytes without knowing registry identities or routing.
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Load the component and validate its imports and configuration.
    async fn load(&self, artifact: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError>;
}

/// Exports returned by loading a component. Each invocation owns its own runtime state.
#[derive(Clone)]
pub struct Loaded {
    /// Adapter-provided description, not a plugin identity.
    pub summary: String,
    /// Plugin-owned configuration documentation, retained with the loaded generation.
    pub config_schema: serde_json::Value,
    /// Credential declarations used by transport, wiring and generated documentation.
    pub credentials: Vec<Credential>,
    /// A missing environment requirement; the code still loads and retains its health.
    pub unavailable: Option<Failure>,
    /// Self-check required before a deployment is admitted.
    pub lifecycle: Arc<dyn Lifecycle>,
    /// Context policy and its profile options, fixed with the loaded generation.
    pub composer: Option<LoadedComposer>,
    /// Messaging protocol mapper when supplied by the component.
    pub channel: Option<Arc<dyn Channel>>,
    /// Completion interface when supplied by the component.
    pub completion: Option<Arc<dyn Provider>>,
    /// Embedding interface when supplied by the component.
    pub embedding: Option<Arc<dyn Embedding>>,
    /// Decision interface when supplied by the component.
    pub decision: Option<Arc<dyn Decision>>,
    /// Declared tool definitions and their common invocation interface.
    pub tools: Option<LoadedTools>,
}

/// A composer's option declaration and invocation, loaded from the same content.
#[derive(Clone)]
pub struct LoadedComposer {
    /// Profile options accepted by this generation.
    pub schema: enco_core::InputSchema,
    /// Writes requests from the host's fixed facts.
    pub adapter: Arc<dyn Composer>,
}

/// Tool declarations are evaluated once with the generation's accepted configuration.
#[derive(Clone)]
pub struct LoadedTools {
    /// Model-facing definitions fixed with this generation.
    pub specs: Vec<ToolSpec>,
    /// Executes a tool with the definition supplied by its caller.
    pub adapter: Arc<dyn Tools>,
}

/// A plugin's tools interface, independent of the registry-assigned code identity.
#[async_trait]
pub trait Tools: Send + Sync {
    /// Execute a declared tool and wait for any started work to become quiescent on cancellation.
    async fn call(
        &self,
        ctx: CallContext,
        spec: &ToolSpec,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome;
}

/// A declared credential; transport validates placement and alone reads its value.
#[derive(Clone, serde::Serialize)]
pub struct Credential {
    /// Name used in the owner's secret mapping.
    pub name: String,
    /// Fixed service origin, or the service selected by the caller.
    pub origin: Option<String>,
    /// Whether the mapping must be present.
    pub required: bool,
    /// The location populated by transport.
    pub placement: Placement,
}

/// A request header carrying a credential.
#[derive(Clone, serde::Serialize)]
pub struct HeaderPlacement {
    /// Header name overwritten by the transport.
    pub name: String,
    /// Literal text before the secret value.
    pub prefix: String,
}

/// Where transport alone writes a credential value.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Placement {
    /// One header populated with a prefix and the secret.
    Header(HeaderPlacement),
    /// Insert immediately after the matching request-path prefix.
    Path(Vec<String>),
}

/// Why a component cannot be loaded, attributed to its code or to the settings it was given.
#[derive(Debug, Clone, serde::Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum LoadError {
    /// This host cannot compile, link or instantiate the component, or its declarations are invalid.
    #[error("{0}")]
    Component(String),
    /// The settings cannot be used with this component.
    #[error("{0}")]
    Settings(String),
    /// The component reported a failure while describing itself or its tools.
    #[error("{}: {}", .0.code, .0.message)]
    Plugin(Failure),
}

/// Component lifecycle shared by all exported interfaces.
#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// Run a local self-check without contacting external services.
    async fn probe(&self) -> Result<(), Failure>;
}
