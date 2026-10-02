use async_trait::async_trait;
use enco_core::*;

/// Completion boundary. Implementations own timeouts and per-invocation state.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Complete a frozen request with the caller's settings and separately supplied credential.
    /// Dropping the future cancels local work.
    async fn complete(
        &self,
        settings: &ProviderSettings,
        api_key: Option<&str>,
        request: ProviderRequest,
    ) -> Result<Completion, Failure>;
}

/// Resolved request; all content comes from the frozen plan and its Log references.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRequest {
    /// Canonical messages in request order.
    pub messages: Vec<Message>,
    /// Frozen definitions disclosed to the model.
    pub tools: Vec<ToolSpec>,
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
