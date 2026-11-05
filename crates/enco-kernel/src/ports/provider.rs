use crate::{Store, StoreError};
use async_trait::async_trait;
use enco_core::*;
use std::sync::Arc;

/// Completion boundary. Implementations own timeouts and per-invocation state.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Complete a frozen request; the transport resolves credentials from the settings.
    ///
    /// With `record`, the service's response is recorded there before it is decoded, and the
    /// answer names that record; failing to record it is a store error, not an answer. Dropping
    /// the future cancels local work.
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
        record: Option<Recording>,
    ) -> Result<Answered, StoreError>;
}

/// Where a Provider records the responses it decodes, under the generation that decodes them.
#[derive(Clone)]
pub struct Recording {
    /// The Provider generation that decodes the response.
    pub generation: GenerationId,
    /// The store that keeps the response body and the mapping record.
    pub store: Arc<dyn Store>,
}

/// How a completion call ended, with the recorded call that decoded the service's response.
#[derive(Debug)]
pub struct Answered {
    /// The completion, or why there is none.
    pub result: Result<Completion, CompleteError>,
    /// The decoding call's record; absent when no response arrived, the request could not be
    /// built, or the call had no `record`.
    pub origin: Option<MappingOrigin>,
}

/// Why a completion did not happen.
#[derive(Debug, Clone, PartialEq)]
pub enum CompleteError {
    /// The request failed.
    Failed(Failure),
    /// The service turned the request away as too large; a smaller request may succeed.
    Overflow(Overflow),
}

impl From<Failure> for CompleteError {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

impl CompleteError {
    /// The failure a caller that cannot shrink its request reports.
    pub fn into_failure(self) -> Failure {
        match self {
            Self::Failed(failure) => failure,
            Self::Overflow(overflow) => overflow.failure(),
        }
    }
}

/// A canonical model request. Session requests resolve a frozen, recorded plan;
/// registry probes contain only fixed validation content.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRequest<I = ImageData> {
    /// Canonical messages in request order.
    pub messages: Vec<Message<I>>,
    /// Frozen definitions disclosed to the model.
    pub tools: Vec<DisclosedTool>,
    /// The most tokens the reply may use.
    pub max_output_tokens: std::num::NonZeroU32,
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
