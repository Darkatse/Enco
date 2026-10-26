//! The situations that need attention, as facts without prose.
//!
//! Each kind carries only the facts that describe it; `attention` decides which situations arise,
//! and the renderer words them.
use super::Steps;
use crate::development::EntryError;
use enco_core::{
    ConfigurationRef, Failure, IntentPath, PluginActivation, PluginName, RegistrationId,
};
use enco_kernel::{Cause, SettingsError, UnservedUse};
use serde::Serialize;

/// One situation that needs attention, with the steps to take next.
#[derive(Serialize)]
pub struct Attention<'a> {
    /// What the situation is, with its facts; in JSON they sit beside `next`.
    #[serde(flatten)]
    pub kind: AttentionKind<'a>,
    /// What to do about it.
    pub next: Steps,
}

/// The kind of situation, with only the facts that describe it.
///
/// Fact templates receive only this value, so they cannot infer prose from the steps.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionKind<'a> {
    /// A plugin directory in the home with no registered plugin under its name.
    New {
        /// The plugin directory.
        path: IntentPath,
    },
    /// The home has unregistered changes to code or configuration.
    Unregistered {
        /// `config.toml` or the plugin directory that changed.
        path: IntentPath,
        /// The latest registration, which the home is compared with.
        registration: RegistrationId,
    },
    /// Source changed, and only a build can tell whether its code did.
    Unbuilt {
        /// The plugin directory whose source changed.
        path: IntentPath,
        /// The latest registration, which the home is compared with.
        registration: RegistrationId,
    },
    /// A registered plugin's directory was deleted from the home, and the deletion is not
    /// registered.
    Removed {
        /// The deleted plugin directory.
        path: IntentPath,
        /// Registration that registered the deleted source.
        registration: RegistrationId,
        /// Whether the plugin still serves calls.
        running: bool,
        /// Whether the host has factory code for the plugin, which runs once the removal is
        /// registered.
        factory: bool,
    },
    /// The registered generation failed, and the plugin runs an older one instead.
    RolledBack {
        /// The plugin directory.
        path: IntentPath,
        /// The older generation that runs.
        running: PluginActivation,
        /// The registered generation, which failed.
        registered: PluginActivation,
        /// The failure, by what the failed generation introduced.
        cause: GenerationFailure<'a>,
    },
    /// What runs for the plugin differs from what is registered in one respect.
    Diverged {
        /// The plugin directory.
        path: IntentPath,
        /// How what runs differs.
        divergence: Divergence<'a>,
    },
    /// The plugin serves no calls.
    Stopped {
        /// The plugin directory.
        path: IntentPath,
        /// Why it serves no calls.
        cause: StopCause<'a>,
    },
    /// The running `config.toml` is not the registered one, as after a rollback.
    ConfigurationDiverged {
        /// The configuration that runs.
        running: ConfigurationRef,
        /// The configuration the latest registration selected.
        registered: ConfigurationRef,
    },
    /// Registering this host's factory code for the plugin was rejected; every registration
    /// retries it.
    FollowRejected {
        /// The plugin directory.
        path: IntentPath,
        /// Why the registration was rejected.
        causes: &'a [Cause],
        /// The generation the plugin keeps running; absent when it serves no calls.
        running: Option<PluginActivation>,
    },
    /// A configured use cannot be served, for a reason other than its plugin being unable to run.
    UnservedUse {
        /// The use, the endpoint it names, and why it cannot be served.
        usage: &'a UnservedUse,
    },
    /// A table of the running configuration that this host cannot interpret; what it configures
    /// stays stopped.
    InvalidTable {
        /// The table and what is wrong with it.
        error: &'a SettingsError,
    },
    /// `config.toml` or a plugin directory cannot be read, so it cannot be compared with what is
    /// registered.
    Unreadable {
        /// `config.toml` or the plugin directory.
        path: IntentPath,
        /// The file or directory that failed, and why.
        error: &'a EntryError,
    },
    /// The `plugins` directory cannot be listed, so no plugin's home layer is known.
    PluginsUnreadable {
        /// The file or directory that failed, and why.
        error: &'a EntryError,
    },
    /// The owner turned safe mode on: only recovery tools are offered, and plugins with factory
    /// code run it.
    SafeMode,
    /// The Scheduler stopped after a failure, and reminders are not delivered.
    SchedulerStopped {
        /// The failure that stopped it.
        reason: &'a str,
    },
    /// A Session stopped after a storage failure, and messages to it are rejected.
    SessionStopped {
        /// The Session's name.
        name: &'a str,
        /// The storage failure that stopped it.
        reason: &'a str,
    },
}

/// One way in which what runs for a plugin differs from what is registered.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Divergence<'a> {
    /// The plugin runs under a name from an earlier registration.
    Name {
        /// The name it runs under.
        running: &'a PluginName,
        /// The registered name.
        registered: &'a PluginName,
        /// Registration that gave the registered name.
        registration: RegistrationId,
    },
    /// The plugin runs code other than the registered code, which is not marked failed.
    Code {
        /// The generation that runs.
        running: PluginActivation,
        /// The registered generation.
        registered: PluginActivation,
    },
    /// The plugin runs settings other than the registered settings, which are not marked failed.
    Settings {
        /// The generation that runs.
        running: PluginActivation,
        /// The registered generation.
        registered: PluginActivation,
    },
    /// A registration removed the plugin, but mechanical recovery, such as an owner rollback,
    /// runs it again.
    Unregistered {
        /// The generation that runs.
        running: PluginActivation,
        /// Registration that removed the plugin.
        removed_by: RegistrationId,
    },
}

/// A recorded failure of a generation, attributed by what the generation introduced.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerationFailure<'a> {
    /// The generation introduced code from a plugin directory.
    Code {
        /// The failure recorded for the generation.
        failure: &'a Failure,
    },
    /// The generation introduced new settings and reuses existing code.
    Settings {
        /// The failure recorded for the generation.
        failure: &'a Failure,
    },
    /// The generation runs the host's factory code, which only a host update changes.
    Factory {
        /// The failure recorded for the generation.
        failure: &'a Failure,
    },
}

/// Why a plugin serves no calls.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopCause<'a> {
    /// The registered generation failed, and no older generation can run.
    Failed {
        /// The failure, by what the failed generation introduced.
        cause: GenerationFailure<'a>,
    },
    /// A generation is selected, but this host cannot serve it.
    Unavailable {
        /// Why this host cannot serve it.
        causes: &'a [Cause],
    },
    /// A generation is registered, but none is selected to run.
    NotRunning {
        /// The registered generation.
        registered: PluginActivation,
    },
}
