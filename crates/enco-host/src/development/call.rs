//! Each development tool has one declaration: its name, description, schema, effect and parser.
use super::{
    AgentReason, ApplyArguments, HISTORY_PAGE_SIZE, HistoryQuery, RenameArguments, RewindArguments,
    ScaffoldArguments,
};
use crate::limits::BUILD_DIAGNOSTICS;
use enco_core::{
    Effect, Failure, InputSchema, REASON_PLACEHOLDER, Reason, SchemaError, ToolSpec,
    parse_arguments,
};
use enco_kernel::RECENT_REGISTRATIONS;
use serde::Serialize;
use serde_json::{Map, Value};

/// Name of the tool that registers every change in the home.
pub const APPLY: &str = "self_apply";
/// Name of the tool that lists registrations and compares versions of the home's files.
pub const HISTORY: &str = "self_history";
/// Name of the tool that writes an earlier registration's files back into the home.
pub const REWIND: &str = "self_rewind";
/// Name of the tool that renames a plugin together with its directory.
pub const RENAME: &str = "plugin_rename";
/// Name of the tool that creates a tool plugin in the home.
pub const SCAFFOLD: &str = "plugin_scaffold";
/// Name of the tool that shows Enco's state and what needs attention.
pub const STATUS: &str = "self_status";

/// A development tool that this crate implements; `self_status` belongs to the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevelopmentTool {
    /// `self_apply`.
    Apply,
    /// `self_history`.
    History,
    /// `self_rewind`.
    Rewind,
    /// `plugin_rename`.
    Rename,
    /// `plugin_scaffold`.
    Scaffold,
}

/// One parsed call of a development tool.
///
/// Suggestions use `AgentReason` where the Agent writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DevelopmentCall<R = Reason> {
    /// A call of `self_apply`.
    Apply(ApplyArguments<R>),
    /// A call of `self_history`.
    History(HistoryQuery),
    /// A call of `self_rewind`.
    Rewind(RewindArguments),
    /// A call of `plugin_rename`.
    Rename(RenameArguments<R>),
    /// A call of `plugin_scaffold`.
    Scaffold(ScaffoldArguments),
}

/// The arguments of `self_status`, which takes none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatusArguments {}

/// A suggested call, serialized only where a document or JSON result is written.
#[derive(Clone, Debug, Serialize)]
pub struct Invocation {
    /// The name of the tool to call.
    pub tool: &'static str,
    /// The arguments to call it with.
    pub arguments: SuggestedArguments,
    /// Arguments the Agent writes itself, such as `reason`; the suggestion holds placeholders.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agent_supplies: Vec<&'static str>,
}

/// The arguments of a suggested call.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum SuggestedArguments {
    /// The arguments of a development tool, with placeholders where the Agent writes.
    Development(DevelopmentCall<AgentReason>),
    /// The arguments of `self_status`.
    Status(StatusArguments),
}

impl DevelopmentTool {
    /// Every development tool.
    pub const ALL: [Self; 5] = [
        Self::Apply,
        Self::History,
        Self::Rewind,
        Self::Rename,
        Self::Scaffold,
    ];

    /// The name the model calls the tool by.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Apply => APPLY,
            Self::History => HISTORY,
            Self::Rewind => REWIND,
            Self::Rename => RENAME,
            Self::Scaffold => SCAFFOLD,
        }
    }

    /// The tool called `name`, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// Parse the arguments of a call to this tool.
    ///
    /// Execution uses this parser and no other.
    pub fn parse(self, arguments: Map<String, Value>) -> Result<DevelopmentCall, Failure> {
        match self {
            Self::Apply => parse_arguments(arguments).map(DevelopmentCall::Apply),
            Self::History => parse_arguments(arguments).map(DevelopmentCall::History),
            Self::Rewind => parse_arguments(arguments).map(DevelopmentCall::Rewind),
            Self::Rename => parse_arguments(arguments).map(DevelopmentCall::Rename),
            Self::Scaffold => parse_arguments(arguments).map(DevelopmentCall::Scaffold),
        }
    }

    /// The tool's declaration: its name, description, argument schema and effect.
    pub fn spec(self) -> Result<ToolSpec, SchemaError> {
        let name = self.name();
        match self {
            Self::Apply => ToolSpec::of::<ApplyArguments>(
                name,
                format!(
                    r#"Register all changes in the home as one registration. Changed plugin sources are built, and config.toml is validated together with them. If nothing changed, nothing is registered. New plugin code starts on trial and becomes healthy once its probe and first calls succeed; if it fails on trial, Enco runs the previous code again and the registered source stays as it is. Registering config.toml replaces the whole registered document. Sources built with an earlier SDK are rebuilt. The result lists the registration, the builds with up to {BUILD_DIAGNOSTICS} compiler diagnostics and the full log path, and the checks that ran. If another change was registered first, nothing is registered; read {STATUS} and call again."#
                ),
                Effect::SideEffect,
            ),
            Self::History => ToolSpec::of::<HistoryQuery>(
                name,
                format!(
                    r#"Read registration history and compare versions of config.toml and the plugin directories.
- {{}} lists the latest {HISTORY_PAGE_SIZE} registrations, newest first. For older ones, pass {{"before": <oldest number shown>}}.
- {{"to": 12}} shows registration 12 and its diff from the registration before it.
- {{"to": "home"}} shows unregistered changes: the files in the home compared with the latest registration.
- {{"from": "home", "to": 11}} shows what rewinding the listed paths to registration 11 would write.
- {{"from": 9, "to": 12}} compares two registrations.
A result starts with a summary and ends with line diffs. A long result is saved to a file, and the result gives its path."#
                ),
                Effect::ReadOnly,
            ),
            Self::Rewind => ToolSpec::of::<RewindArguments>(
                name,
                format!(
                    r#"Write an earlier registration's version of config.toml or plugin directories back into the home. Nothing is registered: review with {HISTORY} {{"to": "home"}}, then call {APPLY}.
- {{"registration": 11}} rewinds every path whose registered version changed after registration 11. If that would overwrite unregistered changes, nothing is written and the result lists them.
- {{"registration": 11, "paths": ["plugins/web"]}} rewinds only the listed paths and overwrites their unregistered changes.
Plugins keep their current names; undo a rename with {RENAME}. A plugin deleted since then comes back under its old name as a new plugin. Ignored files and .git are left untouched."#
                ),
                Effect::Idempotent,
            ),
            Self::Rename => ToolSpec::of::<RenameArguments>(
                name,
                format!(
                    r#"Rename a plugin: move plugins/<name> to plugins/<to> and register the new name in one step. Its history, code and settings stay, and unregistered edits move with the directory. Factory plugins and names that the running configuration uses cannot be renamed. Do not move the directory yourself; {APPLY} then rejects the change."#
                ),
                Effect::SideEffect,
            ),
            Self::Scaffold => ToolSpec::of::<ScaffoldArguments>(
                name,
                format!(
                    r#"Create a new tool plugin in plugins/<name> with source, Cargo files, ignore rules and a README. Nothing is registered: edit src/lib.rs, then call {APPLY}."#
                ),
                Effect::SideEffect,
            ),
        }
    }
}

/// The declaration of `self_status`, whose handler belongs to the application.
pub fn self_status_spec() -> ToolSpec {
    ToolSpec {
        name: STATUS.into(),
        description: format!(
            r#"Show Enco's state now. First comes what needs attention, each item with what to do next. A table then shows config.toml and every plugin in three layers: running (the generation in use), registered (the generation its latest registration chose) and home (the files in the Enco home). The latest {RECENT_REGISTRATIONS} registrations, Sessions and channels follow. A registration is one accepted change; a generation is one registered version of a plugin or of the configuration. Reading status builds and changes nothing."#
        ),
        input_schema: InputSchema::empty(),
        effect: Effect::ReadOnly,
    }
}

impl<R> DevelopmentCall<R> {
    /// The tool this call is for.
    pub fn tool(&self) -> DevelopmentTool {
        match self {
            Self::Apply(_) => DevelopmentTool::Apply,
            Self::History(_) => DevelopmentTool::History,
            Self::Rewind(_) => DevelopmentTool::Rewind,
            Self::Rename(_) => DevelopmentTool::Rename,
            Self::Scaffold(_) => DevelopmentTool::Scaffold,
        }
    }
}

impl DevelopmentCall<AgentReason> {
    pub(crate) fn register() -> Self {
        Self::Apply(ApplyArguments {
            reason: AgentReason,
        })
    }
}

impl Invocation {
    /// Suggest this call, naming the arguments the Agent writes itself.
    pub fn new(call: DevelopmentCall<AgentReason>) -> Self {
        let agent_supplies = match &call {
            DevelopmentCall::Apply(_) | DevelopmentCall::Rename(_) => vec!["reason"],
            DevelopmentCall::History(_)
            | DevelopmentCall::Rewind(_)
            | DevelopmentCall::Scaffold(_) => Vec::new(),
        };
        Self {
            tool: call.tool().name(),
            arguments: SuggestedArguments::Development(call),
            agent_supplies,
        }
    }

    /// Suggest reading `self_status`.
    pub fn status() -> Self {
        Self {
            tool: STATUS,
            arguments: SuggestedArguments::Status(StatusArguments {}),
            agent_supplies: Vec::new(),
        }
    }
}

impl Serialize for AgentReason {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REASON_PLACEHOLDER)
    }
}
