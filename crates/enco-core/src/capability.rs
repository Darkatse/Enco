use crate::{ContentHash, GenerationId, NodeId, PluginId, RegistrationId};
use serde::{Deserialize, Serialize};

/// A node-qualified capability, displayed as `name@node`; the model sees only `name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityId {
    /// Node exporting this capability.
    pub node: NodeId,
    /// Tool name within the exporting node.
    pub name: String,
}

/// A plugin's code, named within the plugin; WIT calls it `plugin-code`.
///
/// Records that already belong to one plugin hold this; [`CodeRef`] adds the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginCode {
    /// The code and settings one registration brought for the plugin.
    Generation {
        /// The registration that brought it.
        registration: RegistrationId,
    },
    /// Code shipped with a host, run with empty settings; it is host content, never registered.
    Factory {
        /// Content address of the embedded component; hosts with the same bytes agree on it.
        artifact: ContentHash,
    },
}

impl PluginCode {
    /// The registered generation of `plugin` this names, absent for factory code.
    pub fn generation(self, plugin: PluginId) -> Option<GenerationId> {
        match self {
            Self::Generation { registration } => Some(GenerationId {
                registration,
                plugin,
            }),
            Self::Factory { .. } => None,
        }
    }
}

/// Names code the way text for the owner and the Agent does, with the plugin as the subject:
/// `registration 15`, by the registration that brought it, or `factory code`.
impl std::fmt::Display for PluginCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generation { registration } => write!(f, "registration {registration}"),
            Self::Factory { .. } => f.write_str("factory code"),
        }
    }
}

/// A plugin's code, named across plugins: the exact code an invocation used or a plugin runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CodeRef {
    /// The plugin the code belongs to.
    pub plugin: PluginId,
    /// Which of the plugin's code.
    pub code: PluginCode,
}

impl CodeRef {
    /// The registered generation this names, absent for factory code.
    pub fn generation(self) -> Option<GenerationId> {
        self.code.generation(self.plugin)
    }
}

/// Names the code as [`PluginCode`] does; the plugin is the subject of the sentence.
impl std::fmt::Display for CodeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.code.fmt(f)
    }
}

impl From<GenerationId> for CodeRef {
    fn from(id: GenerationId) -> Self {
        Self {
            plugin: id.plugin,
            code: PluginCode::Generation {
                registration: id.registration,
            },
        }
    }
}

/// Code linked into the host binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCode {
    /// Identity of the linked native capability.
    pub name: String,
    /// Version of the linked native crate.
    pub version: String,
}

/// The code behind a tool call: a plugin's, or the host's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCode {
    /// A plugin's exported tool.
    Plugin(CodeRef),
    /// A tool linked into the host binary.
    Native(NativeCode),
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.node)
    }
}
