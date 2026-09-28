use async_trait::async_trait;
use enco_core::*;

/// Adapter boundary for completions and embeddings. Implementations own timeouts.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Code serving this request, recorded before calling it.
    fn code(&self) -> CodeRef;
    /// Complete a frozen request. Dropping the future cancels local work.
    async fn complete(&self, request: ProviderRequest) -> Result<Completion, Failure>;
    /// Embed texts in input order; used by host context sources, not the kernel.
    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure>;
}

/// Resolved request; all content comes from the frozen plan and its Log references.
#[derive(Debug, Clone, PartialEq)]
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
