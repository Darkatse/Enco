use crate::Budget;
use enco_core::{AttemptPurpose, ProviderSettings};

/// A caller's endpoint and credentials, independent of the chosen code generation.
#[derive(Clone)]
pub struct Endpoint {
    /// Registered plugin name whose completion export serves this endpoint.
    pub plugin: String,
    /// Durable parameters copied into AttemptStarted.
    pub settings: ProviderSettings,
    /// Credential read once by the composition root; never serialized.
    pub api_key: Option<String>,
    /// Context window and reserved output of this model.
    pub budget: Budget,
}

/// Session policy sampled once at the start of a Round.
#[derive(Clone)]
pub struct Profile {
    /// Endpoint used for ordinary replies.
    pub reply: Endpoint,
    /// Endpoint used for summarizing history.
    pub compaction: Endpoint,
    /// Whether every reply plan must disclose all lifeline tools.
    pub requires_lifeline: bool,
}

impl Profile {
    /// Resolve the endpoint for this Attempt without consulting mutable routing.
    pub fn endpoint(&self, purpose: AttemptPurpose) -> &Endpoint {
        match purpose {
            AttemptPurpose::Reply => &self.reply,
            AttemptPurpose::Compaction => &self.compaction,
        }
    }
}
