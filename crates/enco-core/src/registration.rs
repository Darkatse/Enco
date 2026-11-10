//! Accepted changes and the runtime references they own.
use crate::{
    CallId, ContentHash, DateTime, PluginCode, PluginId, PluginName, RegistrationId, SessionId, Utc,
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
}

/// The code one registration activated for a plugin, named relative to both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegisteredCode {
    /// The generation this registration brought; its origin says where its code came from.
    Generation,
    /// A host's factory code run as itself, with empty settings: the registration chose to
    /// follow it.
    Factory {
        /// Content address of the embedded component.
        artifact: ContentHash,
    },
}

impl RegisteredCode {
    /// The plugin's code this names, given the registration that activated it.
    pub fn code(self, registration: RegistrationId) -> PluginCode {
        match self {
            Self::Generation => PluginCode::Generation { registration },
            Self::Factory { artifact } => PluginCode::Factory { artifact },
        }
    }
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

impl CodeIntent {
    /// The code intent a registration chose for a plugin; new settings choose none.
    ///
    /// `brought` is the origin of the generation the registration brought for it, absent when it
    /// activated factory code itself, which is choosing to follow factory code.
    pub fn chosen(brought: Option<&crate::Origin>) -> Option<Self> {
        match brought {
            None => Some(Self::Factory),
            Some(crate::Origin::Code { intent }) => Some(*intent),
            Some(crate::Origin::Settings { .. }) => None,
        }
    }
}

/// One node's mutable references for a plugin, owned exclusively by the registry.
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

/// A plugin's current or retained code on this node, with the registration that wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginActivation {
    /// The generation a registration brought, written by that registration.
    Generation {
        /// The registration that brought the generation and wrote this reference.
        registration: RegistrationId,
    },
    /// A host's factory code, run with empty settings; a rollback target may keep an earlier
    /// host's.
    Factory {
        /// Content address of the embedded component.
        artifact: ContentHash,
        /// The registration that chose it; absent when no registration did, as when mechanical
        /// recovery reached it.
        registration: Option<RegistrationId>,
    },
}

impl PluginActivation {
    /// The reference a registration writes when it activates `code`.
    pub fn registered(registration: RegistrationId, code: RegisteredCode) -> Self {
        match code {
            RegisteredCode::Generation => Self::Generation { registration },
            RegisteredCode::Factory { artifact } => Self::Factory {
                artifact,
                registration: Some(registration),
            },
        }
    }

    /// The plugin's code it names.
    pub fn code(self) -> PluginCode {
        match self {
            Self::Generation { registration } => PluginCode::Generation { registration },
            Self::Factory { artifact, .. } => PluginCode::Factory { artifact },
        }
    }

    /// The registration that wrote it; absent only for factory code reached by recovery.
    pub fn registration(self) -> Option<RegistrationId> {
        match self {
            Self::Generation { registration } => Some(registration),
            Self::Factory { registration, .. } => registration,
        }
    }
}
