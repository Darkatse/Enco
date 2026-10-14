use crate::{ContentHash, DateTime, Failure, GenerationId, PluginId, Utc};
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
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Component embedded in the host binary.
    Factory,
    /// Component explicitly deployed by the owner or agent.
    Deployed,
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
    /// Complete configuration content; declarations are derived by its owners.
    pub document: serde_json::Value,
    /// Observation time for display, never ordering.
    pub created_at: DateTime<Utc>,
}
