use serde::{Deserialize, Serialize};

/// Per-call provider settings recorded before invocation. Credential values belong only to the host transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// Service endpoint for this call.
    pub base_url: String,
    /// Service-specific model identifier.
    pub model: String,
    /// Environment variable the host transport reads at invocation time.
    pub api_key_env: Option<String>,
    /// Adapter options included in the recorded request parameters.
    pub options: serde_json::Value,
}
