use enco_core::{IntentPath, PluginName, Reason, RegistrationId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A suggested call's reason, which the Agent writes. It serializes as the placeholder that
/// reason parsing rejects, so a suggestion copied unchanged cannot register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentReason;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyArguments<R = Reason> {
    /// Why this change is registered, in one sentence; history shows it.
    pub reason: R,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameArguments<R = Reason> {
    /// The plugin's current name.
    pub name: PluginName,
    #[schemars(description = format!("The new name: {}.", PluginName::RULE))]
    pub to: PluginName,
    /// Why the plugin is renamed, in one sentence; history shows it.
    pub reason: R,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RewindArguments {
    /// The registration whose versions are written back.
    pub registration: RegistrationId,
    /// The paths to rewind, each "config.toml" or "plugins/<name>" with the plugin's current name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<IntentPath>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScaffoldArguments {
    #[schemars(description = format!("The new plugin's name: {}.", PluginName::RULE))]
    pub name: PluginName,
}

impl RewindArguments {
    pub(crate) fn path(registration: RegistrationId, path: IntentPath) -> Self {
        Self {
            registration,
            paths: Some(vec![path]),
        }
    }
}
