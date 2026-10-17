//! Accepted changes and the runtime references they own.
use crate::{DateTime, GenerationId, PluginId, RegistrationId, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One accepted external change, also the unit of owner rollback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationRecord {
    /// Structural commit order, independent of Git history and wall clocks.
    pub id: RegistrationId,
    /// Commit on the registered-intent chain; its parent is the preceding registration.
    pub revision: String,
    /// Name versions written only when an identity acquires, changes or releases its name.
    pub names: BTreeMap<PluginId, RegisteredName>,
    /// Code activations written by this change, independently of name changes.
    pub code: BTreeMap<PluginId, RegisteredCode>,
    /// Observation time for display.
    pub created_at: DateTime<Utc>,
}

/// One accepted name change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredName {
    /// Directory and routing name, or no name after removal.
    pub name: Option<String>,
    /// Name version actually in effect before this change, for the same identity.
    pub previous: Option<RegistrationId>,
}

/// One accepted code activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredCode {
    /// Code activated by this change.
    pub generation: GenerationId,
    /// Code intent accepted here; settings and artifacts leave it unchanged.
    pub intent: Option<CodeIntent>,
}

/// The owner's choice of code source, independent of the resulting component bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeIntent {
    /// Build and maintain the source directory in the home.
    Directory,
    /// Follow the implementation supplied by the host.
    Factory,
}

/// Mutable references owned exclusively by the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRecord {
    /// Immutable installation identity; names may change or be reused.
    pub id: PluginId,
    /// This identity's current name row, including a row recording removal.
    pub named_by: Option<RegistrationId>,
    /// Code used for new calls; absence means the plugin is stopped.
    pub active: Option<PluginActivation>,
    /// Retained healthy code available for mechanical recovery.
    pub rollback_target: Option<PluginActivation>,
}

/// A current or retained code reference, distinct from the generation's creation provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginActivation {
    /// Target code and settings.
    pub generation: GenerationId,
    /// Registration which wrote this reference; absent only for direct factory recovery.
    pub registration: Option<RegistrationId>,
}
