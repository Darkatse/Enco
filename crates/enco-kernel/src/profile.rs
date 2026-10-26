use crate::{Budget, Endpoint};
use enco_core::PluginName;
use serde_json::{Map, Value};

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
    /// Plugin and typed invocation options used to compose this Session's requests.
    pub composer: ComposerSettings,
    /// Ordered candidates for ordinary replies.
    pub reply: Endpoints,
    /// Ordered candidates for summarizing history.
    pub compaction: Endpoints,
    /// Whether every reply plan must disclose all lifeline tools.
    pub requires_lifeline: bool,
    /// Allowed tool names; absent exposes all available tools, including future additions.
    pub tools: Option<std::collections::BTreeSet<String>>,
    /// Whether composition receives the owner's standing instructions and memories.
    pub owner_context: bool,
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

/// A profile's selected composer and its invocation options.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerSettings {
    /// Registered name of the plugin exporting composition.
    pub plugin: PluginName,
    /// Options checked against the generation selected for each composition.
    #[serde(default)]
    pub options: ComposerOptions,
}

/// A profile's composer options: a JSON object whose shape the composer's schema declares.
pub type ComposerOptions = Map<String, Value>;
