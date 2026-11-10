use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

macro_rules! id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(ulid::Ulid);
        impl $name {
            /// Allocate a globally unique identity, independent of ordering.
            pub fn new() -> Self {
                Self(ulid::Ulid::generate())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({ "type": "string" })
            }
        }

        impl FromStr for $name {
            type Err = ulid::DecodeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }
    };
}
id!(SessionId, "Durable identity of a conversation and its Log.");
id!(EventId, "Input identity used for delivery deduplication.");
id!(RunId, "Identity of one Run of a Session.");
id!(
    RoundId,
    "Identity of a model request and its tool settlements."
);
id!(
    AttemptId,
    "Identity of one provider attempt, including failed attempts."
);
id!(
    CallId,
    "Host-assigned identity of one proposed tool invocation."
);
id!(ScheduleId, "Identity of a durable schedule.");
id!(MemoryId, "Identity of an editable memory.");
id!(NodeId, "Persistent identity of an Enco installation.");
id!(
    PluginId,
    "Persistent identity assigned when a plugin name is registered."
);

impl EventId {
    /// The identity of the reminder for one occurrence of a schedule.
    ///
    /// Every node and every retry derives the same identity from the same schedule and planned
    /// time, so the Inbox and the Log accept the occurrence once. The planned time, not the
    /// clock, decides it.
    pub fn reminder(schedule: ScheduleId, due_at: crate::DateTime<crate::Utc>) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&schedule.0.to_bytes());
        hasher.update(&due_at.timestamp().to_le_bytes());
        hasher.update(&due_at.timestamp_subsec_nanos().to_le_bytes());
        let mut random = [0_u8; 16];
        random[6..].copy_from_slice(&hasher.finalize().as_bytes()[..10]);
        // The time part only orders identities; no schedule is due before 1970.
        let millis = u64::try_from(due_at.timestamp_millis()).unwrap_or(0);
        Self(ulid::Ulid::from_parts(millis, u128::from_be_bytes(random)))
    }
}

impl RoundId {
    /// The full Round identity used to seed deterministic composition randomness.
    pub fn as_u128(self) -> u128 {
        self.0.into()
    }
}

/// A plugin generation's identity: the registration that brought its content, and the plugin.
///
/// A registration brings at most one generation per plugin, so every node derives the same
/// identity from the same registration. A configuration generation is identified by its
/// registration alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GenerationId {
    /// Registration that brought content the host does not have.
    pub registration: RegistrationId,
    /// Plugin whose history contains the generation.
    pub plugin: PluginId,
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "plugin {} from registration {}",
            self.plugin, self.registration
        )
    }
}

/// Registry-global registration number; one accepted change may contain several generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistrationId(pub u64);

impl schemars::JsonSchema for RegistrationId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RegistrationId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "integer", "minimum": 1 })
    }
}

impl fmt::Display for RegistrationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Ownership generation; single-node execution uses one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Epoch(pub u64);

/// Sequence assigned by a Session actor, starting at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seq(pub u64);

/// Structural Log ordering; wall clocks never determine this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogPos {
    /// Ownership generation.
    pub epoch: Epoch,
    /// Sequence within the generation.
    pub seq: Seq,
}
