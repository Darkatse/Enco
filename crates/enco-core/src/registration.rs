//! Accepted changes and the runtime references they own.
use crate::{
    CallId, ContentHash, DateTime, GenerationId, PluginId, PluginName, RegistrationId, SessionId,
    Utc,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One accepted external change, also the unit of owner rollback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationRecord {
    /// Structural commit order, independent of wall clocks.
    pub id: RegistrationId,
    /// Caller responsible for this accepted change.
    pub initiator: Initiator,
    /// Name versions written only when an identity acquires, changes or releases its name.
    pub names: BTreeMap<PluginId, RegisteredName>,
    /// Code activations written by this change, independently of name changes.
    pub code: BTreeMap<PluginId, RegisteredCode>,
    /// Observation time for display.
    pub created_at: DateTime<Utc>,
}

/// Provenance of an accepted change; Agent reasoning remains in its Session log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Initiator<S = SessionId> {
    /// An explicit owner command.
    Owner {
        /// The owner's explanation, if any.
        reason: Option<Reason>,
    },
    /// Initial registration or following a new host's factory code.
    Host,
    /// A tool call made by an Agent.
    Agent {
        /// Session containing the reasoning and tool invocation.
        session: S,
        /// Invocation which requested registration.
        call: CallId,
        /// Explanation given before changing registered behavior.
        reason: Reason,
    },
}

impl<S> Initiator<S> {
    /// The caller's explanation; host changes have no caller-provided reason.
    pub fn reason(&self) -> Option<&Reason> {
        match self {
            Self::Owner { reason } => reason.as_ref(),
            Self::Agent { reason, .. } => Some(reason),
            Self::Host => None,
        }
    }
}

/// A nonempty, bounded line explaining an intentional change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Reason(String);

impl Reason {
    /// Maximum number of Unicode characters in a registration reason.
    pub const MAX_CHARS: usize = 512;

    /// The original explanation, without normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reserved marker in suggested calls; it is never an acceptable registration reason.
pub const REASON_PLACEHOLDER: &str = "<why>";

/// A reason that cannot be stored as a single bounded line.
#[derive(Debug, thiserror::Error)]
pub enum InvalidReason {
    /// The caller has not replaced the suggested-call marker.
    #[error("Replace {REASON_PLACEHOLDER} with your reason.")]
    Placeholder,
    /// A reason must fit on one bounded, nonempty line.
    #[error(
        "reason must be a nonempty line of at most {} characters",
        Reason::MAX_CHARS
    )]
    Line,
}

impl std::str::FromStr for Reason {
    type Err = InvalidReason;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim() == REASON_PLACEHOLDER {
            return Err(InvalidReason::Placeholder);
        }
        if text.trim().is_empty()
            || text.contains(['\r', '\n'])
            || text.chars().count() > Self::MAX_CHARS
        {
            return Err(InvalidReason::Line);
        }
        Ok(Self(text.to_owned()))
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl schemars::JsonSchema for Reason {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Reason".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "minLength": 1, "maxLength": Self::MAX_CHARS })
    }
}

impl<'de> Deserialize<'de> for Reason {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Reason;

    #[test]
    fn reasons_are_bounded_nonempty_lines_at_the_json_boundary() {
        for text in [
            "",
            "  ",
            super::REASON_PLACEHOLDER,
            "one\ntwo",
            "one\rtwo",
            &"字".repeat(Reason::MAX_CHARS + 1),
        ] {
            assert!(serde_json::from_value::<Reason>(serde_json::json!(text)).is_err());
        }
        let text = "字".repeat(Reason::MAX_CHARS);
        assert!(
            matches!(serde_json::from_value::<Reason>(serde_json::json!(text)), Ok(reason) if reason.as_str() == text)
        );
    }
}

/// One accepted name change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredName {
    /// Directory and routing name, or no name after removal.
    pub name: Option<PluginName>,
    /// Name version actually in effect before this change, for the same identity.
    pub previous: Option<RegistrationId>,
}

/// One accepted code activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredCode {
    /// Code activated by this change.
    pub generation: GenerationId,
    /// Code intent accepted here; settings and artifacts leave it unchanged.
    pub intent: Option<CodeIntent>,
}

/// The owner's choice of code source, independent of the resulting component bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeIntent {
    /// Build and maintain the source directory in the home.
    Directory {
        /// Content-addressed source tree owned by the store.
        source: ContentHash,
    },
    /// Follow the implementation supplied by the host.
    Factory,
}

/// Mutable references owned exclusively by the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRecord {
    /// Immutable installation identity; names may change or be reused.
    pub id: PluginId,
    /// This identity's current name row, including a row recording removal.
    pub named_by: Option<RegistrationId>,
    /// Code used for new calls; absence means the plugin is stopped.
    pub active: Option<PluginActivation>,
    /// Retained healthy code available for mechanical recovery.
    pub rollback_target: Option<PluginActivation>,
}

/// A current or retained code reference, distinct from the generation's creation provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginActivation {
    /// Target code and settings.
    pub generation: GenerationId,
    /// Registration which wrote this reference; absent only for direct factory recovery.
    pub registration: Option<RegistrationId>,
}
