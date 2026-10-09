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
    /// Plugin configuration, separate from per-call model settings.
    pub config: serde_json::Value,
    /// How this generation entered the registry.
    pub origin: Origin,
    /// Whether this generation remains eligible for use.
    pub status: GenerationStatus,
    /// The reason this generation was marked failed; absent for trial and healthy records.
    pub failure: Option<Failure>,
    /// Observation time for display, never ordering.
    pub created_at: DateTime<Utc>,
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
