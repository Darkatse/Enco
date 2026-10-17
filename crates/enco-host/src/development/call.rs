//! Each development tool has one declaration: its name, description, schema, effect and parser.
use super::{
    AgentReason, ApplyArguments, HISTORY_PAGE_SIZE, HistoryQuery, RenameArguments, RewindArguments,
    ScaffoldArguments,
};
use crate::limits::BUILD_DIAGNOSTICS;
use enco_core::{Effect, Failure, REASON_PLACEHOLDER, Reason, ToolSpec, closed_object_schema};
use enco_kernel::RECENT_REGISTRATIONS;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const APPLY: &str = "self_apply";
pub const HISTORY: &str = "self_history";
pub const REWIND: &str = "self_rewind";
pub const RENAME: &str = "plugin_rename";
pub const SCAFFOLD: &str = "plugin_scaffold";
pub const STATUS: &str = "self_status";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevelopmentTool {
    Apply,
    History,
    Rewind,
    Rename,
    Scaffold,
}

/// One parsed call of a development tool. Suggestions use `AgentReason` where the Agent writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DevelopmentCall<R = Reason> {
    Apply(ApplyArguments<R>),
    History(HistoryQuery),
    Rewind(RewindArguments),
    Rename(RenameArguments<R>),
    Scaffold(ScaffoldArguments),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusArguments {}

/// A suggested call, serialized only where a document or JSON result is written.
#[derive(Clone, Debug, Serialize)]
pub struct Invocation {
    pub tool: &'static str,
    pub arguments: SuggestedArguments,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agent_supplies: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum SuggestedArguments {
    Development(DevelopmentCall<AgentReason>),
    Status(StatusArguments),
}

impl DevelopmentTool {
    pub const ALL: [Self; 5] = [
        Self::Apply,
        Self::History,
        Self::Rewind,
        Self::Rename,
        Self::Scaffold,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Apply => APPLY,
            Self::History => HISTORY,
            Self::Rewind => REWIND,
            Self::Rename => RENAME,
            Self::Scaffold => SCAFFOLD,
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The parser execution uses, and the only one.
    pub fn parse(self, arguments: Map<String, Value>) -> Result<DevelopmentCall, Failure> {
        let arguments = Value::Object(arguments);
        match self {
            Self::Apply => serde_json::from_value(arguments).map(DevelopmentCall::Apply),
            Self::History => serde_json::from_value(arguments).map(DevelopmentCall::History),
            Self::Rewind => serde_json::from_value(arguments).map(DevelopmentCall::Rewind),
            Self::Rename => serde_json::from_value(arguments).map(DevelopmentCall::Rename),
            Self::Scaffold => serde_json::from_value(arguments).map(DevelopmentCall::Scaffold),
        }
        .map_err(|error| crate::tools::args::invalid(error.to_string()))
    }

    pub fn spec(self) -> ToolSpec {
        let reason_chars = Reason::MAX_CHARS;
        let side = json!({"oneOf": [{"type":"integer", "minimum":1}, {"type":"string", "enum":["workspace"]}]});
        let reason = json!({"type":"string", "minLength":1, "maxLength":reason_chars});
        let (description, properties, required, effect) = match self {
            Self::Apply => (
                format!(
                    r#"Register all changes in the workspace as one registration. Changed plugin sources are built, and config.toml is validated together with them. Requires a one-sentence reason of at most {reason_chars} characters; history shows it. If nothing changed, nothing is registered. New plugin code starts on trial and becomes healthy once its probe and first calls succeed; if it fails on trial, Enco runs the previous code again and the registered source stays as it is. Registering config.toml replaces the whole registered document. Sources built with an earlier SDK are rebuilt. The result lists the registration, the builds with up to {BUILD_DIAGNOSTICS} compiler diagnostics and the full log path, and the checks that ran. If another change was registered first, nothing is registered; read {STATUS} and call again."#
                ),
                json!({"reason": reason}),
                vec!["reason"],
                Effect::SideEffect,
            ),
            Self::History => (
                format!(
                    r#"Read registration history and compare versions of config.toml and the plugin directories. Each side of a comparison is a registration number or "workspace".
- {{}} lists the latest {HISTORY_PAGE_SIZE} registrations, newest first. For older ones, pass {{"before": <oldest number shown>}}.
- {{"to": 12}} shows registration 12 and its diff from the registration before it.
- {{"to": "workspace"}} shows unregistered changes: the workspace compared with the latest registration.
- {{"from": "workspace", "to": 11}} shows what rewinding the listed paths to registration 11 would write.
- {{"from": 9, "to": 12}} compares two registrations.
A result starts with a summary and ends with line diffs. A long result is saved to a file, and the result gives its path."#
                ),
                json!({"from":side, "to":side, "before":{"type":"integer", "minimum":1}}),
                vec![],
                Effect::ReadOnly,
            ),
            Self::Rewind => (
                format!(
                    r#"Write an earlier registration's version of config.toml or plugin directories back into the workspace. Nothing is registered: review with {HISTORY} {{"to": "workspace"}}, then call {APPLY}.
- {{"registration": 11}} rewinds every path whose registered version changed after registration 11. If that would overwrite unregistered changes, nothing is written and the result lists them.
- {{"registration": 11, "paths": ["plugins/web"]}} rewinds only the listed paths and overwrites their unregistered changes. A path is "config.toml" or "plugins/<name>" with the plugin's current name.
Plugins keep their current names; undo a rename with {RENAME}. A plugin deleted since then comes back under its old name as a new plugin. Ignored files and .git are left untouched."#
                ),
                json!({"registration":{"type":"integer", "minimum":1}, "paths":{"type":"array", "items":{"type":"string"}}}),
                vec!["registration"],
                Effect::Idempotent,
            ),
            Self::Rename => (
                format!(
                    r#"Rename a plugin: move plugins/<name> to plugins/<to> and register the new name in one step. Its history, code and settings stay, and unregistered edits move with the directory. Requires a one-sentence reason of at most {reason_chars} characters. Factory plugins and names that the running configuration uses cannot be renamed. Do not move the directory yourself; {APPLY} then rejects the change."#
                ),
                json!({"name":{"type":"string"}, "to":{"type":"string"}, "reason": reason}),
                vec!["name", "to", "reason"],
                Effect::SideEffect,
            ),
            Self::Scaffold => (
                format!(
                    r#"Create a new tool plugin in plugins/<name> with source, Cargo files, ignore rules and a README. Nothing is registered: edit src/lib.rs, then call {APPLY}."#
                ),
                json!({"name":{"type":"string"}}),
                vec!["name"],
                Effect::SideEffect,
            ),
        };
        ToolSpec {
            name: self.name().into(),
            description,
            effect,
            input_schema: closed_object_schema(properties, &required),
        }
    }
}

/// The declaration of `self_status`, whose handler belongs to the application.
pub fn self_status_spec() -> ToolSpec {
    ToolSpec {
        name: STATUS.into(),
        description: format!(
            r#"Show Enco's state now. First comes what needs attention, each item with what to do next. A table then shows config.toml and every plugin in three layers: running (the generation in use), registered (the generation its latest registration chose) and workspace (the files in the Enco home). The latest {RECENT_REGISTRATIONS} registrations, Sessions and channels follow. A registration is one accepted change; a generation is one registered version of a plugin or of the configuration. Reading status builds and changes nothing."#
        ),
        input_schema: closed_object_schema(json!({}), &[]),
        effect: Effect::ReadOnly,
    }
}

impl StatusArguments {
    pub fn parse(arguments: Map<String, Value>) -> Result<Self, Failure> {
        serde_json::from_value(Value::Object(arguments))
            .map_err(|error| crate::tools::args::invalid(error.to_string()))
    }
}

impl<R> DevelopmentCall<R> {
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
