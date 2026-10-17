//! One clause per cause, embedded after a colon; the text of a failure is quoted as recorded.
use super::code;
use enco_core::Failure;
use enco_kernel::{
    Cause, ENDPOINT_TABLE, LoadError, PLUGIN_SETTINGS_TABLE, PROFILE_TABLE, Referrer, ToolOwner,
};

pub(super) fn causes(causes: &[Cause]) -> String {
    causes.iter().map(cause).collect::<Vec<_>>().join("; ")
}

fn cause(cause: &Cause) -> String {
    let settings = |plugin: &str| code(&format!("[{}.{plugin}]", PLUGIN_SETTINGS_TABLE.name()));
    let endpoint = |name: &str| code(&format!("[{}.{name}]", ENDPOINT_TABLE.name()));
    match cause {
        Cause::Load { plugin, error, .. } => load(plugin, error),
        Cause::Probe {
            plugin, failure, ..
        } => format!(
            "{} failed its self-check: {}",
            code(plugin),
            recorded(failure)
        ),
        Cause::NotLoaded { plugin } => {
            format!("the selected generation of {} is not loaded", code(plugin))
        }
        Cause::Environment { plugin, failure } => format!(
            "{} lacks a requirement from its environment: {}",
            code(plugin),
            recorded(failure)
        ),
        Cause::ToolConflict { plugin, tool, with } => match with {
            ToolOwner::Host => format!(
                "{} exports the tool {}, which a host tool already has",
                code(plugin),
                code(tool)
            ),
            ToolOwner::Plugins(others) => format!(
                "{} and {} export the same tool {}",
                code(plugin),
                codes(others.iter().map(|other| other.as_str())),
                code(tool)
            ),
        },
        Cause::UnknownPlugin {
            user,
            endpoint: name,
            plugin,
        } => format!(
            "{} uses {}, which names {}, but no plugin runs under that name",
            code(user),
            endpoint(name),
            code(plugin)
        ),
        Cause::PluginUnavailable {
            user,
            endpoint: name,
            plugin,
        } => format!(
            "{} uses {}, whose plugin {} cannot run",
            code(user),
            endpoint(name),
            code(plugin)
        ),
        Cause::Use {
            user,
            endpoint: name,
            failure,
        } => format!(
            "{} cannot use {}: {}",
            code(user),
            endpoint(name),
            recorded(failure)
        ),
        Cause::Table(error) => {
            format!("{}: {}", code(&format!("[{}]", error.table)), error.message)
        }
        Cause::ProfileInUse { profile, sessions } => format!(
            "Sessions {} use {}, which is missing",
            codes(sessions.iter().map(String::as_str)),
            code(&format!("[{}.{profile}]", PROFILE_TABLE.name()))
        ),
        Cause::SettingsWithoutPlugin { plugin } => {
            format!("{} names no plugin", settings(plugin))
        }
        Cause::SettingsWithoutCode { plugin } => format!(
            "{} changed, but no code of {} runs to use it",
            settings(plugin),
            code(plugin)
        ),
        Cause::NameReserved { name } => format!("the host reserves the name {}", code(name)),
        Cause::NameTaken { name } => format!("another plugin is registered as {}", code(name)),
        Cause::Referenced { name, by } => format!(
            "{} is still used by {}",
            code(name),
            by.iter()
                .map(|referrer| match referrer {
                    Referrer::User { user } => code(user),
                    Referrer::Settings => settings(name),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn load(plugin: &str, error: &LoadError) -> String {
    match error {
        LoadError::Component(reason) => format!("{} cannot be loaded: {reason}", code(plugin)),
        LoadError::Settings(reason) => format!(
            "{} rejects {}: {reason}",
            code(plugin),
            code(&format!("[{}.{plugin}]", PLUGIN_SETTINGS_TABLE.name()))
        ),
        LoadError::Plugin(failure) => format!(
            "{} failed while loading: {}",
            code(plugin),
            recorded(failure)
        ),
    }
}

fn codes<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.map(code).collect::<Vec<_>>().join(", ")
}

pub(super) fn recorded(failure: &Failure) -> String {
    format!("{} ({})", failure.message, code(&failure.code))
}
