use async_trait::async_trait;
use enco_core::*;
use tokio_util::sync::CancellationToken;

/// Supplies one owner's facts for a Round, in that owner's type; never called in safe mode.
#[async_trait]
pub trait ContextSource: Send + Sync {
    /// What this source supplies.
    type Supply: Send;
    /// Stop external waits on cancellation, then finish any local write before returning.
    async fn contribute(&self, query: &ContextQuery) -> Result<Self::Supply, ContextError>;
}

/// The memories offered for one Round, and what could not be offered.
#[derive(Debug, Clone, Default)]
pub struct OfferedMemories {
    /// Pinned memories first, then those recalled for the latest input, most relevant first.
    pub memories: Vec<Memory>,
    /// Degraded or skipped recall, with reasons the plan records.
    pub omitted: Vec<Omission>,
}

/// Shared input to a Round's context sources.
pub struct ContextQuery {
    /// Session for which content is being assembled.
    pub session: SessionRecord,
    /// Latest consumed input; does not imply source data is unchanged.
    pub latest_event: Option<Event>,
    /// Whether this Round consumed any new Inbox input.
    pub new_input: bool,
    /// Child of the current Run cancellation token.
    pub cancel: CancellationToken,
}

/// A context source failed to supply its authoritative content.
#[derive(Debug, thiserror::Error)]
#[error("context source: {0}")]
pub struct ContextError(pub String);
