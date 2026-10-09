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

/// Registry-global generation number, ordered by commit rather than time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenerationId(pub u64);

impl fmt::Display for GenerationId {
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
