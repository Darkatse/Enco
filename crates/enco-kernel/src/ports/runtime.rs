use crate::{Embedding, Provider};
use async_trait::async_trait;
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
    /// Completion interface when supplied by the component.
    pub completion: Option<Arc<dyn Provider>>,
    /// Embedding interface when supplied by the component.
    pub embedding: Option<Arc<dyn Embedding>>,
}

/// Compilation, import validation or component description failed.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LoadError(pub String);
