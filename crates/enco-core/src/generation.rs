use crate::{ContentHash, DateTime, Failure, GenerationId, PluginId, RegistrationId, Utc};
use serde::{Deserialize, Serialize};

/// One immutable generation identity, with a mutable availability status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRecord {
    /// Registry-assigned structural sequence number.
    pub id: GenerationId,
    /// Plugin whose history contains this generation.
    pub plugin: PluginId,
    /// Content-addressed component bytes.
    pub artifact: ContentHash,
    /// Owner-selected plugin configuration and credential names, fixed together with the code.
    pub settings: PluginSettings,
    /// How this generation entered the registry.
    #[serde(flatten)]
    pub origin: Origin,
    /// Whether this generation remains eligible for use.
    pub status: GenerationStatus,
    /// The reason this generation was marked failed; absent for trial and healthy records.
    pub failure: Option<Failure>,
    /// Observation time for display, never ordering.
    pub created_at: DateTime<Utc>,
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

/// Source of a registered generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum Origin {
    /// Component embedded in the host binary.
    Factory,
    /// Accepted code intent, from source or the current host's factory implementation.
    Code {
        /// Accepted change that supplied this code.
        registration: RegistrationId,
    },
    /// New settings paired with the currently active code.
    Settings {
        /// Accepted change that supplied these settings.
        registration: RegistrationId,
        /// Generation whose code is reused.
        based_on: GenerationId,
    },
    /// A component supplied directly, independently of source intent.
    Artifact {
        /// Accepted change that supplied this component.
        registration: RegistrationId,
    },
}

impl Origin {
    /// The accepted change that produced this content; factory content belongs to the host.
    pub fn registration(self) -> Option<RegistrationId> {
        match self {
            Self::Factory => None,
            Self::Code { registration }
            | Self::Settings { registration, .. }
            | Self::Artifact { registration } => Some(registration),
        }
    }
}

/// Eligibility for use and automatic recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    /// Active under observation; not yet eligible as a rollback target.
    Trial,
    /// Eligible to become active or serve as a rollback target.
    Healthy,
    /// Unavailable; redeploying its bytes creates a new identity.
    Failed,
}

/// A committed automatic rollback, also accepted as a Session input when applicable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RolledBack {
    /// Registered plugin name.
    pub plugin: String,
    /// Failed trial generation.
    pub from: GenerationId,
    /// Replacement, or none when no healthy generation remains.
    pub to: Option<GenerationId>,
    /// Plugin fault which triggered recovery.
    pub failure: Failure,
}

/// An immutable registered configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigGeneration {
    /// Global registry sequence number, shared with plugin generations.
    pub id: GenerationId,
    /// Accepted change which produced this generation.
    pub registration: RegistrationId,
    /// Complete configuration content; declarations are derived by its owners.
    pub document: serde_json::Value,
    /// Configuration which was active immediately before this one was registered.
    pub previous: Option<GenerationId>,
    /// Observation time for display, never ordering.
    pub created_at: DateTime<Utc>,
}
