//! Each cause names the part of the intent it concerns; callers repair from that, never its text.
use super::*;

/// The part of the owner's intent that a cause points to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum IntentPart {
    /// The source of a plugin.
    Code {
        /// Plugin whose code is at fault.
        plugin: PluginName,
    },
    /// The `[plugin.<name>]` table of a plugin.
    Settings {
        /// Plugin whose settings are at fault.
        plugin: PluginName,
    },
    /// A requested plugin name.
    Name {
        /// The name that cannot be used.
        name: PluginName,
    },
    /// Configuration that still refers to a plugin name.
    References {
        /// The referenced name.
        name: PluginName,
    },
    /// A configured invocation as one caller needs it.
    Use {
        /// Configured consumer, such as `profile default.reply`.
        user: String,
        /// Configuration entry named by that consumer.
        target: UseRef,
    },
    /// A top-level configuration table, including a name for named entries.
    Table {
        /// Table such as `memory` or `endpoint.chat`.
        table: String,
    },
    /// Code shipped with this host; registration cannot change it.
    Factory {
        /// Plugin whose factory code is at fault.
        plugin: PluginName,
    },
    /// A requirement the owner provides outside the intent, such as a secret value.
    Environment {
        /// Plugin missing the requirement.
        plugin: PluginName,
    },
    /// A plugin that cannot run; its own status says why.
    Plugin {
        /// The plugin that cannot run, by its current name.
        plugin: PluginName,
    },
}

/// What in the configuration refers to a plugin name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "referrer", rename_all = "snake_case")]
pub enum Referrer {
    /// A configured caller naming the plugin.
    User {
        /// The user, such as `profile default.reply`.
        user: String,
    },
    /// The plugin's own `[plugin.<name>]` table.
    Settings,
}

/// What a candidate generation changes, which decides the part its failure concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Proposed {
    /// Source or a supplied component.
    Code,
    /// New settings for the running code.
    Settings,
    /// Code shipped with this host.
    Factory,
}

/// A failure attributed to the part of the intent it concerns.
#[derive(Debug, Clone, serde::Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// The candidate could not be loaded with its settings.
    #[error("plugin {plugin}: {error}")]
    Load {
        /// Plugin whose candidate failed.
        plugin: PluginName,
        /// What the candidate changes.
        candidate: Proposed,
        /// Failure reported while loading.
        error: LoadError,
    },
    /// The loaded candidate failed its local self-check.
    #[error("plugin {plugin}: probe failed: {}: {}", failure.code, failure.message)]
    Probe {
        /// Plugin whose candidate failed.
        plugin: PluginName,
        /// What the candidate changes.
        candidate: Proposed,
        /// Failure returned by the probe.
        failure: Failure,
    },
    /// The selected generation is not loaded on this host.
    #[error("plugin {plugin}: the selected generation is not loaded")]
    NotLoaded {
        /// Plugin without loaded code.
        plugin: PluginName,
    },
    /// The component loads but lacks a requirement from its environment.
    #[error("plugin {plugin}: {}: {}", failure.code, failure.message)]
    Environment {
        /// Plugin missing the requirement.
        plugin: PluginName,
        /// Failure reported by the component.
        failure: Failure,
    },
    /// An exported tool name is already taken.
    #[error("plugin {plugin}: tool {tool} is also exported by {with}")]
    ToolConflict {
        /// Plugin exporting the tool.
        plugin: PluginName,
        /// Conflicting tool name.
        tool: String,
        /// The host or the other plugins exporting it.
        with: ToolOwner,
    },
    /// A configured invocation names a plugin that does not run.
    #[error("{user} ({target}): no plugin named {plugin} runs")]
    UnknownPlugin {
        /// Configured consumer.
        user: String,
        /// Configuration entry named by that consumer.
        target: UseRef,
        /// Plugin named by the invocation.
        plugin: PluginName,
    },
    /// A configured invocation names a plugin that runs but cannot serve calls.
    #[error("{user} ({target}): plugin {plugin} cannot run")]
    PluginUnavailable {
        /// Configured consumer.
        user: String,
        /// Configuration entry named by that consumer.
        target: UseRef,
        /// Plugin named by the invocation.
        plugin: PluginName,
    },
    /// A configured invocation cannot be served.
    #[error("{user} ({target}): {}: {}", failure.code, failure.message)]
    Use {
        /// Configured consumer.
        user: String,
        /// Configuration entry named by that consumer.
        target: UseRef,
        /// Interface check or real call failure.
        failure: Failure,
    },
    /// A configuration table cannot be interpreted.
    #[error("{0}")]
    Table(SettingsError),
    /// Removing a profile would strand the Sessions that use it.
    #[error("profile {profile} is still used by Sessions {}", .sessions.join(", "))]
    ProfileInUse {
        /// Profile missing from the configuration.
        profile: String,
        /// Names of the Sessions using it.
        sessions: Vec<String>,
    },
    /// Settings name a plugin that this registration does not have.
    #[error("[{}.{plugin}]: no plugin has this name", PLUGIN_SETTINGS_TABLE.name())]
    SettingsWithoutPlugin {
        /// Name in the settings table.
        plugin: PluginName,
    },
    /// Settings changed for a plugin with no running code to pair them with.
    #[error("[{}.{plugin}]: no code is running; register source together with these settings", PLUGIN_SETTINGS_TABLE.name())]
    SettingsWithoutCode {
        /// Plugin whose settings changed.
        plugin: PluginName,
    },
    /// The host reserves this name for its factory code.
    #[error("plugin name {name} is reserved by the host")]
    NameReserved {
        /// Reserved name.
        name: PluginName,
    },
    /// Another plugin already has this name.
    #[error("plugin name {name} is already registered")]
    NameTaken {
        /// Taken name.
        name: PluginName,
    },
    /// The configuration still refers to the name.
    #[error("plugin name {name} is still referenced by {by:?}")]
    Referenced {
        /// Referenced name.
        name: PluginName,
        /// What refers to it.
        by: Vec<Referrer>,
    },
}

impl Cause {
    /// The part of the intent that must change, or that the owner must provide.
    pub fn part(&self) -> IntentPart {
        match self {
            Self::Load {
                plugin,
                candidate,
                error,
            } => candidate.part(plugin, matches!(error, LoadError::Settings(_))),
            Self::Probe {
                plugin, candidate, ..
            } => candidate.part(plugin, false),
            Self::NotLoaded { plugin } | Self::ToolConflict { plugin, .. } => IntentPart::Code {
                plugin: plugin.clone(),
            },
            Self::Environment { plugin, .. } => IntentPart::Environment {
                plugin: plugin.clone(),
            },
            Self::UnknownPlugin { user, target, .. } | Self::Use { user, target, .. } => {
                IntentPart::Use {
                    user: user.clone(),
                    target: target.clone(),
                }
            }
            Self::Table(error) => IntentPart::Table {
                table: error.table.clone(),
            },
            Self::ProfileInUse { profile, .. } => IntentPart::Table {
                table: format!("{}.{profile}", crate::PROFILE_TABLE.name()),
            },
            Self::SettingsWithoutPlugin { plugin } | Self::SettingsWithoutCode { plugin } => {
                IntentPart::Settings {
                    plugin: plugin.clone(),
                }
            }
            Self::NameReserved { name } | Self::NameTaken { name } => {
                IntentPart::Name { name: name.clone() }
            }
            Self::Referenced { name, .. } => IntentPart::References { name: name.clone() },
            Self::PluginUnavailable { plugin, .. } => IntentPart::Plugin {
                plugin: plugin.clone(),
            },
        }
    }
}

impl Proposed {
    fn part(self, plugin: &PluginName, settings_rejected: bool) -> IntentPart {
        let plugin = plugin.clone();
        match self {
            Self::Settings => IntentPart::Settings { plugin },
            _ if settings_rejected => IntentPart::Settings { plugin },
            Self::Factory => IntentPart::Factory { plugin },
            Self::Code => IntentPart::Code { plugin },
        }
    }
}

impl std::fmt::Display for ToolOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Host => f.write_str("a host tool"),
            Self::Plugins(plugins) => f.write_str(
                &plugins
                    .iter()
                    .map(PluginName::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }
    }
}

/// The failure a caller of an unavailable plugin receives; its text is rendered once, here.
pub(super) fn unavailable_failure(causes: &[Cause]) -> Failure {
    Failure {
        code: code::PLUGIN_UNAVAILABLE.into(),
        message: causes
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "),
        retryable: false,
    }
}
