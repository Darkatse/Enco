use async_trait::async_trait;
use enco_core::{Failure, ProviderSettings};

/// Embedding boundary consumed by host memory, independently of completion.
#[async_trait]
pub trait Embedding: Send + Sync {
    /// Embed texts in input order; credentials are never persisted with settings.
    /// Return one nonempty, finite vector per input, all with the same width.
    async fn embed(
        &self,
        settings: &ProviderSettings,
        api_key: Option<&str>,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure>;
}
