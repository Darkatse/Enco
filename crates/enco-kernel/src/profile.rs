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
}

/// Per-purpose endpoints. M11 maps the existing node configuration to one default profile.
#[derive(Clone)]
pub struct Profile {
    /// Endpoint used for ordinary replies.
    pub reply: Endpoint,
    /// Endpoint used for summarizing history.
    pub compaction: Endpoint,
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
