use super::Steps;
use crate::development::EntryError;
use enco_core::{
    ConfigurationRef, Failure, IntentPath, PluginActivation, PluginName, RegistrationId,
};
use enco_kernel::{Cause, SettingsError, UnservedUse};
use serde::Serialize;

#[derive(Serialize)]
pub struct Attention<'a> {
    #[serde(flatten)]
    pub kind: AttentionKind<'a>,
    pub next: Steps,
}

/// Fact templates receive only this value, so they cannot infer prose from the steps.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionKind<'a> {
    New {
        path: IntentPath,
    },
    Unregistered {
        path: IntentPath,
        registration: RegistrationId,
    },
    /// Source changed, and only a build can tell whether its code did.
    Unbuilt {
        path: IntentPath,
        registration: RegistrationId,
    },
    Removed {
        path: IntentPath,
        registration: RegistrationId,
        running: bool,
        factory: bool,
    },
    RolledBack {
        path: IntentPath,
        running: PluginActivation,
        registered: PluginActivation,
        cause: GenerationFailure<'a>,
    },
    Diverged {
        path: IntentPath,
        divergence: Divergence<'a>,
    },
    Stopped {
        path: IntentPath,
        cause: StopCause<'a>,
    },
    ConfigurationDiverged {
        running: ConfigurationRef,
        registered: ConfigurationRef,
    },
    FollowRejected {
        path: IntentPath,
        causes: &'a [Cause],
        running: Option<PluginActivation>,
    },
    UnservedUse {
        usage: &'a UnservedUse,
    },
    InvalidTable {
        error: &'a SettingsError,
    },
    Unreadable {
        path: IntentPath,
        error: &'a EntryError,
    },
    PluginsUnreadable {
        error: &'a EntryError,
    },
    SafeMode,
    SchedulerStopped {
        reason: &'a str,
    },
    SessionStopped {
        name: &'a str,
        reason: &'a str,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Divergence<'a> {
    Name {
        running: &'a PluginName,
        registered: &'a PluginName,
        registration: RegistrationId,
    },
    Code {
        running: PluginActivation,
        registered: PluginActivation,
    },
    Settings {
        running: PluginActivation,
        registered: PluginActivation,
    },
    Unregistered {
        running: PluginActivation,
        removed_by: RegistrationId,
    },
}

/// A recorded failure of a generation, attributed by what the generation introduced.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerationFailure<'a> {
    Code { failure: &'a Failure },
    Settings { failure: &'a Failure },
    Factory { failure: &'a Failure },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopCause<'a> {
    Failed {
        cause: GenerationFailure<'a>,
    },
    Unavailable {
        causes: &'a [Cause],
    },
    /// A generation is registered, but none is selected to run.
    NotRunning {
        registered: PluginActivation,
    },
}
