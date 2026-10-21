use crate::{Budget, Endpoint};

/// A profile's endpoint with the context limits required by completion.
#[derive(Clone)]
pub struct CompletionEndpoint {
    /// Resolved service shared with all endpoint users.
    pub endpoint: Endpoint,
    /// Context window and reserved output of this model.
    pub budget: Budget,
    /// Non-text inputs accepted by this model.
    pub accepts: std::collections::BTreeSet<enco_core::Modality>,
}

/// Session policy sampled once at the start of a Round.
#[derive(Clone)]
pub struct Profile {
    /// Ordered candidates for ordinary replies.
    pub reply: Endpoints,
    /// Ordered candidates for summarizing history.
    pub compaction: Endpoints,
    /// Whether every reply plan must disclose all lifeline tools.
    pub requires_lifeline: bool,
}

/// An ordered, nonempty set of completion candidates accepted by configuration parsing.
#[derive(Clone)]
pub struct Endpoints {
    first: CompletionEndpoint,
    rest: Vec<CompletionEndpoint>,
}

impl Endpoints {
    pub(crate) fn new(first: CompletionEndpoint, rest: Vec<CompletionEndpoint>) -> Self {
        Self { first, rest }
    }

    /// The configured first choice, irrespective of its current availability.
    pub fn first(&self) -> &CompletionEndpoint {
        &self.first
    }

    /// The fallbacks after the first choice, in configured order.
    pub fn rest(&self) -> &[CompletionEndpoint] {
        &self.rest
    }

    /// Candidates in the owner's configured order.
    pub fn iter(&self) -> impl Iterator<Item = &CompletionEndpoint> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}
