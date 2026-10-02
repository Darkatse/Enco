use crate::{ContentHash, DateTime, GenerationId, PluginId, Utc};
use serde::{Deserialize, Serialize};

/// One immutable activation identity, with a mutable availability status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRecord {
    /// Registry-assigned structural sequence number.
    pub id: GenerationId,
    /// Plugin whose history contains this activation.
    pub plugin: PluginId,
    /// Content-addressed component bytes.
    pub artifact: ContentHash,
    /// Plugin configuration, separate from per-call model settings.
    pub config: serde_json::Value,
    /// How this activation entered the registry.
    pub origin: Origin,
    /// Whether this activation remains eligible for use.
    pub status: GenerationStatus,
    /// Observation time for display, never ordering.
    pub created_at: DateTime<Utc>,
}

/// Source of a registered activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Component embedded in the host binary.
    Factory,
    /// Component explicitly deployed by the owner or agent.
    Deployed,
}

/// Eligibility for activation. Trial and health reporting arrive with M14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    /// Eligible for activation and rollback.
    Healthy,
    /// Unavailable; redeploying its bytes creates a new identity.
    Failed,
}
