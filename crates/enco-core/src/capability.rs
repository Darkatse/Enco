use crate::{ContentHash, GenerationId, NodeId, PluginId};
use serde::{Deserialize, Serialize};

/// A node-qualified capability, displayed as `name@node`; the model sees only `name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityId {
    /// Node exporting this capability.
    pub node: NodeId,
    /// Tool name within the exporting node.
    pub name: String,
}

/// Identifies the exact code used by an invocation, or that a plugin runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeRef {
    /// Code and settings that one registration brought for a plugin.
    Generation {
        /// The registration and plugin that identify it.
        id: GenerationId,
    },
    /// Code shipped with a host, run with empty settings; it is host content, never registered.
    Factory {
        /// Plugin the host reserves this code for.
        plugin: PluginId,
        /// Content address of the embedded component; hosts with the same bytes agree on it.
        artifact: ContentHash,
    },
    /// Code linked into the host binary.
    Native {
        /// Identity of the linked native capability or policy.
        name: String,
        /// Version of the linked native crate.
        version: String,
    },
}

impl CodeRef {
    /// The plugin this code belongs to; native code belongs to none.
    pub fn plugin(&self) -> Option<PluginId> {
        match self {
            Self::Generation { id } => Some(id.plugin),
            Self::Factory { plugin, .. } => Some(*plugin),
            Self::Native { .. } => None,
        }
    }

    /// The registered generation, absent for host content.
    pub fn generation(&self) -> Option<GenerationId> {
        match self {
            Self::Generation { id } => Some(*id),
            Self::Factory { .. } | Self::Native { .. } => None,
        }
    }
}

/// Names code the way text for the owner and the Agent does, with the plugin as the subject:
/// `registration 15`, by the registration that brought it, or `factory code`.
impl std::fmt::Display for CodeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generation { id } => write!(f, "registration {}", id.registration),
            Self::Factory { .. } => f.write_str("factory code"),
            Self::Native { name, version } => write!(f, "native {name} {version}"),
        }
    }
}

impl From<GenerationId> for CodeRef {
    fn from(id: GenerationId) -> Self {
        Self::Generation { id }
    }
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.node)
    }
}
