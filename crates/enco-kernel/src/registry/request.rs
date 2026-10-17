//! Content offered to the registry; filesystem and build choices belong to the host.
use super::*;

/// One intentional change. All supplied parts are accepted or rejected together.
pub struct RegistrationRequest {
    /// Commit containing the complete registered intent after this change.
    pub revision: String,
    /// Last accepted registration used to prepare the request; absent only on first boot.
    pub base: Option<RegistrationId>,
    /// A changed configuration document, or the initial document at startup.
    pub configuration: Option<serde_json::Value>,
    /// Code, removal and name changes keyed by their user-facing names.
    pub plugins: BTreeMap<String, PluginChange>,
}

/// One plugin's contribution to an atomic registration.
pub enum PluginChange {
    /// Current embedded code for a factory plugin without a source directory.
    Factory,
    /// A component built from source intent.
    Code(ArtifactSource),
    /// A component supplied independently of source intent.
    Artifact(Vec<u8>),
    /// Remove the identity associated with the registered source name.
    Remove,
    /// Change a registered name without changing code or settings.
    Rename(String),
}

/// Newly built bytes or a content address already known to the store.
pub enum ArtifactSource {
    /// Fresh component bytes, persisted only after acceptance.
    Bytes(Vec<u8>),
    /// A retained artifact, or unchanged intent whose bytes need not be loaded.
    Stored(ContentHash),
}

impl ArtifactSource {
    /// Address the offered code without loading it or changing retention.
    pub fn hash(&self) -> ContentHash {
        match self {
            Self::Bytes(bytes) => ContentHash::of(bytes),
            Self::Stored(hash) => *hash,
        }
    }
}

/// The result of one accepted registration, with the evidence gathered before acceptance.
#[derive(serde::Serialize)]
pub struct Registered {
    /// The accepted registration.
    pub registration: RegistrationRecord,
    /// Newly registered plugin content.
    pub generations: Vec<GenerationRecord>,
    /// Newly registered configuration, when present.
    pub configuration: Option<ConfigGeneration>,
    /// Calls which verified new endpoint uses.
    pub probes: Vec<Probe>,
    /// Configured users affected by this registration.
    pub users: Vec<String>,
    /// Post-commit cleanup errors; the registration has already been accepted.
    pub warnings: Vec<String>,
}

/// A completed owner rollback, which may stop plugins when no recoverable code remains.
#[derive(serde::Serialize)]
pub struct Rollback {
    /// Registration whose still-effective parts were restored.
    pub registration: RegistrationId,
    /// Runtime references after recovery.
    pub plugins: Vec<PluginRecord>,
    /// Configuration activated by the rollback, when it changed.
    pub configuration: Option<GenerationId>,
    /// Post-commit cleanup errors; the recovered references are already active.
    pub warnings: Vec<String>,
}
