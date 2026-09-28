use async_trait::async_trait;
use enco_core::*;
use tokio_util::sync::CancellationToken;

/// Supplies current candidate content; never called in safe mode.
#[async_trait]
pub trait ContextSource: Send + Sync {
    /// Stop external waits on cancellation, then finish any local write before returning.
    async fn contribute(&self, query: &ContextQuery) -> Result<Contribution, ContextError>;
}

/// Shared input to a Round's context sources.
pub struct ContextQuery {
    /// Session for which content is being assembled.
    pub session: SessionRecord,
    /// Latest consumed input; does not imply source data is unchanged.
    pub latest_event: Option<Event>,
    /// Child of the current Run cancellation token.
    pub cancel: CancellationToken,
}

/// A context source failed to supply its authoritative content.
#[derive(Debug, thiserror::Error)]
#[error("context source: {0}")]
pub struct ContextError(pub String);
