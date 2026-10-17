use enco_core::{IntentPath, PluginName, Reason, RegistrationId};
use serde::{Deserialize, Serialize};

/// A suggested call's reason, which the Agent writes. It serializes as the placeholder that
/// reason parsing rejects, so a suggestion copied unchanged cannot register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentReason;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyArguments<R = Reason> {
    pub reason: R,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameArguments<R = Reason> {
    pub name: PluginName,
    pub to: PluginName,
    pub reason: R,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewindArguments {
    pub registration: RegistrationId,
    /// The paths to write, or every path whose registered version changed after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<IntentPath>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaffoldArguments {
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
