use async_trait::async_trait;
use enco_core::*;

/// Completion boundary. Implementations own timeouts and per-invocation state.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Complete a frozen request; the transport resolves credentials from the settings.
    /// Dropping the future cancels local work.
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure>;
}

/// A canonical model request. Session requests resolve a frozen, recorded plan;
/// registry probes contain only fixed validation content.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRequest<I = ImageData> {
    /// Canonical messages in request order.
    pub messages: Vec<Message<I>>,
    /// Frozen definitions disclosed to the model.
    pub tools: Vec<DisclosedTool>,
    /// Optional output limit.
    pub max_output_tokens: Option<u32>,
}

/// Provider output with host-assigned call identities.
#[derive(Debug, Clone)]
pub struct Completion {
    /// Assistant message, including opaque provider extensions.
    pub message: Message,
    /// Token usage reported by the provider.
    pub usage: Usage,
    /// Stop condition reported by the provider.
    pub stop: StopReason,
}
