use crate::{CodeRef, ContentHash, Failure, GenerationId, PluginId, PluginName, RegistrationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Immutable plugin content that one registration brought: the artifact, its settings and
/// where the code came from.
///
/// It is intent, the same on every node; whether it runs well is each node's [`Health`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRecord {
    /// The registration that brought it, and its plugin.
    pub id: GenerationId,
    /// Content-addressed component bytes.
    pub artifact: ContentHash,
    /// Owner-selected plugin configuration and credential names, fixed together with the code.
    pub settings: PluginSettings,
    /// Where the code came from.
    #[serde(flatten)]
    pub origin: Origin,
}

/// The complete `[plugin.<name>]` table. Only `config` is visible to the plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginSettings {
    /// Plugin-owned configuration, accepted by its declaration function.
    pub config: serde_json::Value,
    /// Credential names mapped to environment variable names, never their values.
    pub secrets: std::collections::BTreeMap<String, String>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            config: serde_json::json!({}),
            secrets: std::collections::BTreeMap::new(),
        }
    }
}

/// Where a generation's code came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum Origin {
    /// The registration's code intent supplied it: a built source directory, or the host's
    /// factory code with non-empty settings.
    Code,
    /// New settings paired with the code that ran when they were registered.
    Settings {
        /// The code reused, a generation or factory code.
        based_on: CodeRef,
    },
}

/// A generation's standing on one node; another node may judge the same content differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Health {
    /// Active under observation; not yet eligible as a rollback target.
    Trial,
    /// Eligible to become active or serve as a rollback target.
    Healthy,
    /// Unavailable on this node; registering its bytes again brings a new identity.
    Failed {
        /// The attributable fault that marked it failed.
        failure: Failure,
    },
}

impl Health {
    /// The recorded fault, absent unless failed.
    pub fn failure(&self) -> Option<&Failure> {
        match self {
            Self::Failed { failure } => Some(failure),
            Self::Trial | Self::Healthy => None,
        }
    }
}

/// A committed automatic rollback, also accepted as a Session input when applicable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RolledBack {
    /// Running plugin name.
    pub plugin: PluginName,
    /// The generation that failed on trial.
    pub from: GenerationId,
    /// What runs instead: an earlier generation or factory code; absent when nothing can run.
    pub to: Option<CodeRef>,
    /// Plugin fault which triggered recovery.
    pub failure: Failure,
}

/// An immutable registered configuration document; its registration is its identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigGeneration {
    /// Accepted change which produced this generation.
    pub registration: RegistrationId,
    /// Content address of the exact configuration file, including comments.
    pub source: ContentHash,
    /// Complete configuration content; declarations are derived by its owners.
    pub document: serde_json::Value,
}

/// The registered behavior a Round ran with: the active configuration and each running
/// plugin's code.
///
/// Trial promotion leaves it unchanged; a registration or a rollback that changes what runs
/// changes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveCode {
    /// Registration of the active configuration.
    pub configuration: RegistrationId,
    /// Code each running plugin uses.
    pub plugins: BTreeMap<PluginId, CodeRef>,
}
