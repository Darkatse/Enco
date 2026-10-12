use crate::{Budget, Endpoint};
use enco_core::AttemptPurpose;

/// A profile's endpoint with the context limits required by completion.
#[derive(Clone)]
pub struct CompletionEndpoint {
    /// Resolved service shared with all endpoint users.
    pub endpoint: Endpoint,
    /// Context window and reserved output of this model.
    pub budget: Budget,
}

/// Session policy sampled once at the start of a Round.
#[derive(Clone)]
pub struct Profile {
    /// Endpoint used for ordinary replies.
    pub reply: CompletionEndpoint,
    /// Endpoint used for summarizing history.
    pub compaction: CompletionEndpoint,
    /// Whether every reply plan must disclose all lifeline tools.
    pub requires_lifeline: bool,
}

impl Profile {
    /// Resolve the endpoint for this Attempt without consulting mutable routing.
    pub fn endpoint(&self, purpose: AttemptPurpose) -> &CompletionEndpoint {
        match purpose {
            AttemptPurpose::Reply => &self.reply,
            AttemptPurpose::Compaction => &self.compaction,
        }
    }
}
