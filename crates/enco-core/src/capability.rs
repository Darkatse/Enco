use crate::{ContentHash, NodeId};
use serde::{Deserialize, Serialize};

/// A node-qualified capability, displayed as `name@node`; the model sees only `name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityId {
    /// Node exporting this capability.
    pub node: NodeId,
    /// Tool name within the exporting node.
    pub name: String,
}

/// Identifies the exact native code or immutable component used by an invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeRef {
    /// Code linked into the host binary.
    Native {
        /// Identity of the linked native capability or policy.
        name: String,
        /// Version of the linked native crate.
        version: String,
    },
    /// An immutable component addressed by its bytes.
    Wasm {
        /// Content hash of the component bytes.
        artifact: ContentHash,
    },
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.node)
    }
}
