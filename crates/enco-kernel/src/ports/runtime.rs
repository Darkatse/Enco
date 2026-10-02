use crate::{Embedding, Provider};
use async_trait::async_trait;
use enco_core::Failure;
use std::sync::Arc;

/// Compile and describe component bytes without knowing registry identities or routing.
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Load the component and validate its imports and configuration.
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError>;
}

/// Exports held by an activated generation. Each invocation owns its own runtime state.
#[derive(Clone)]
pub struct Loaded {
    /// Adapter-provided description, not a plugin identity.
    pub summary: String,
    /// Self-check required before a deployment is admitted.
    pub lifecycle: Arc<dyn Lifecycle>,
    /// Completion interface when supplied by the component.
    pub completion: Option<Arc<dyn Provider>>,
    /// Embedding interface when supplied by the component.
    pub embedding: Option<Arc<dyn Embedding>>,
}

/// Compilation, import validation or component description failed.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LoadError(pub String);

/// Component lifecycle shared by all exported interfaces.
#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// Run a local self-check without contacting external services.
    async fn probe(&self) -> Result<(), Failure>;
}
