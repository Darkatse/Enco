use serde::{Deserialize, Serialize};

/// Per-call provider settings recorded before invocation. Credentials are passed separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// Service endpoint for this call.
    pub base_url: String,
    /// Service-specific model identifier.
    pub model: String,
    /// Environment variable from which the composition root read the credential.
    pub api_key_env: Option<String>,
    /// Adapter options included in the recorded request parameters.
    pub options: serde_json::Value,
}
