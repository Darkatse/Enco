//! Next steps for the Agent: tool calls with their arguments, fixes it makes itself, and requests
//! to the owner.
//!
//! Each step carries its purpose and the facts it needs, so the renderer only words it.
use crate::development::{
    AgentReason, DevelopmentCall, HistoryPoint, HistoryQuery, Invocation, RenameArguments,
    RewindArguments,
};
use enco_core::{IntentPath, PluginName, RegistrationId};
use enco_kernel::IntentPart;
use serde::Serialize;
use std::path::PathBuf;

/// One thing to do next: a tool call, a fix the Agent makes itself, or a request to the owner.
///
/// The purpose of each action is decided with the situation, never inferred by the renderer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "purpose", rename_all = "snake_case")]
pub enum Step {
    /// A tool call, with the arguments to pass.
    Call(CallStep),
    /// Something the Agent fixes itself, such as a file in the home.
    Fix(FixTarget),
    /// Something only the owner can do; the Agent asks for it.
    AskOwner(OwnerAction),
}

/// A suggested tool call, named by the purpose it serves.
///
/// Each purpose describes exactly one effect, and carries the arguments of its call. In JSON,
/// `call` names the purpose, beside the `tool`, `arguments` and `agent_supplies` of `invocation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallStep {
    /// Read the unregistered changes before registering them.
    Review(HistoryQuery),
    /// Compare two versions, such as the running configuration and the registered one.
    Compare(HistoryQuery),
    /// List registrations, newest first; with a number, only those older than it.
    List(Option<RegistrationId>),
    /// Show one registration in full, with its diff from the registration before it.
    Show(RegistrationId),
    /// Register every change in the home as one registration.
    Register,
    /// Register the home after a plugin directory was deleted, so the deletion takes effect.
    RegisterRemoval,
    /// Return these paths to the latest registration, dropping unregistered changes.
    Discard {
        /// The latest registration, whose versions are written back.
        latest: RegistrationId,
        /// The paths whose unregistered changes are dropped.
        paths: Vec<IntentPath>,
    },
    /// Return a path to the registration whose code or configuration runs.
    KeepRunning(RewindArguments),
    /// Bring deleted plugin directories back from a registration that holds them.
    Restore(RewindArguments),
    /// Write an earlier registration's versions back into the home.
    Rewind(RewindArguments),
    /// Return paths to a chosen registration, overwriting their unregistered changes.
    Overwrite(RewindArguments),
    /// Rename the plugin to the name it runs under, so the registered name matches it.
    RecordName {
        /// The registered name.
        name: PluginName,
        /// The name the plugin runs under.
        to: PluginName,
    },
    /// Rename a plugin together with its directory, and register the new name.
    Rename {
        /// The plugin's current name.
        name: PluginName,
        /// The new name.
        to: PluginName,
    },
    /// Read the status again, to see the state after a change.
    Recheck,
}

/// What the Agent edits; each target is named by the facts that call for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum FixTarget {
    /// The plugin's source in `plugins/<name>`.
    Source {
        /// Plugin whose source needs fixing.
        name: PluginName,
    },
    /// The plugin's `[plugin.<name>]` table in `config.toml`.
    PluginSettings {
        /// Plugin whose settings need fixing.
        name: PluginName,
    },
    /// The `[endpoint.<name>]` table in `config.toml`.
    Endpoint {
        /// The endpoint's name.
        name: String,
    },
    /// A table of `config.toml`, such as `[memory]` or `[endpoint.chat]`.
    Table {
        /// Table name without brackets, such as `memory` or `endpoint.chat`.
        name: String,
    },
    /// A file or directory in the home that cannot be used as it is.
    Entry {
        /// Path of the file or directory.
        path: PathBuf,
    },
    /// A requested plugin name that cannot be used, to replace with another.
    Name {
        /// The name that cannot be used.
        name: PluginName,
    },
    /// References to a plugin name in `config.toml`, to remove.
    References {
        /// The plugin name still referenced.
        name: PluginName,
    },
    /// A repair made with the recovery tools, the only tools offered in safe mode.
    RecoveryTools,
    /// Ignored entries, listed by the refused rewind, that must move out of the paths it writes.
    IgnoredEntries,
    /// A plugin directory to delete from the home.
    RemoveDirectory {
        /// The directory to delete.
        path: IntentPath,
    },
    /// A plugin directory to move back under its registered name.
    MoveDirectory {
        /// Where the directory is now.
        from: IntentPath,
        /// Where it belongs, under the registered name.
        to: IntentPath,
    },
    /// A directory a rewind would write two plugins to; the plugin now there needs a new name.
    SeparateNames {
        /// The directory both plugins would be written to.
        path: IntentPath,
    },
}

/// Something only the owner can do; the Agent asks for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OwnerAction {
    /// Restart Enco; the Agent cannot restart the process it runs in.
    Restart,
    /// Turn safe mode off with `enco safe-mode off`.
    DisableSafeMode,
    /// Repair the host's factory code for a plugin; only a host update changes it.
    FactoryCode {
        /// Plugin whose factory code is at fault.
        name: PluginName,
    },
    /// Provide what a plugin needs from outside the home, such as a secret value or a toolchain.
    Environment {
        /// Plugin that lacks the requirement.
        name: PluginName,
    },
    /// Look into a failure of the host itself, which the Agent cannot repair.
    HostFault,
}

impl CallStep {
    /// The tool call for this step; `agent_supplies` names the arguments the Agent writes itself.
    pub fn invocation(&self) -> Invocation {
        Invocation::new(match self {
            Self::Review(query) | Self::Compare(query) => DevelopmentCall::History(query.clone()),
            Self::List(before) => DevelopmentCall::History(HistoryQuery::List { before: *before }),
            Self::Show(registration) => DevelopmentCall::History(HistoryQuery::Compare {
                from: None,
                to: HistoryPoint::Registration(*registration),
            }),
            Self::Register | Self::RegisterRemoval => DevelopmentCall::register(),
            Self::Discard { latest, paths } => DevelopmentCall::Rewind(RewindArguments {
                registration: *latest,
                paths: Some(paths.clone()),
            }),
            Self::KeepRunning(args)
            | Self::Restore(args)
            | Self::Rewind(args)
            | Self::Overwrite(args) => DevelopmentCall::Rewind(args.clone()),
            Self::RecordName { name, to } | Self::Rename { name, to } => {
                DevelopmentCall::Rename(RenameArguments {
                    name: name.clone(),
                    to: to.clone(),
                    reason: AgentReason,
                })
            }
            Self::Recheck => return Invocation::status(),
        })
    }

    fn purpose(&self) -> &'static str {
        match self {
            Self::Review(_) => "review",
            Self::Compare(_) => "compare",
            Self::List(_) => "list",
            Self::Show(_) => "show",
            Self::Register => "register",
            Self::RegisterRemoval => "register_removal",
            Self::Discard { .. } => "discard",
            Self::KeepRunning(_) => "keep_running",
            Self::Restore(_) => "restore",
            Self::Rewind(_) => "rewind",
            Self::Overwrite(_) => "overwrite",
            Self::RecordName { .. } => "record_name",
            Self::Rename { .. } => "rename",
            Self::Recheck => "recheck",
        }
    }
}

impl Serialize for CallStep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View {
            call: &'static str,
            #[serde(flatten)]
            invocation: Invocation,
        }
        View {
            call: self.purpose(),
            invocation: self.invocation(),
        }
        .serialize(serializer)
    }
}

impl From<CallStep> for Step {
    fn from(call: CallStep) -> Self {
        Self::Call(call)
    }
}

/// The next steps for one situation or failure.
///
/// At most two alternatives; each alternative is performed in order.
#[derive(Clone, Debug, Serialize)]
pub struct Steps {
    /// The steps to take, in order.
    pub first: Vec<Step>,
    /// Steps to take instead of `first`, in order; absent when there is no other choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternative: Option<Vec<Step>>,
}

impl Steps {
    /// Steps to take in order, with no alternative.
    pub(crate) fn sequence(first: Vec<Step>) -> Self {
        Self {
            first,
            alternative: None,
        }
    }

    /// Whether nothing at all is suggested.
    pub(crate) fn is_empty(&self) -> bool {
        self.first.is_empty() && self.alternative.is_none()
    }
}

/// Steps that repair one failure, and whether repeating the failed action is then worthwhile.
pub(crate) struct Repair {
    /// Steps that remove the cause, in order.
    pub(crate) steps: Vec<Step>,
    /// Whether to repeat the failed action after `steps`.
    pub(crate) retry: bool,
}

impl Repair {
    /// No steps; repeating the failed action is all it takes.
    pub(crate) fn retry() -> Self {
        Self {
            steps: Vec::new(),
            retry: true,
        }
    }

    /// These steps, then the failed action again.
    pub(crate) fn then_retry(steps: Vec<Step>) -> Self {
        Self { steps, retry: true }
    }

    /// These steps, without repeating the failed action.
    pub(crate) fn only(steps: Vec<Step>) -> Self {
        Self {
            steps,
            retry: false,
        }
    }

    /// The repair of several failures, deduplicated, retried only when each allows it.
    pub(crate) fn all(repairs: impl IntoIterator<Item = Self>) -> Self {
        let mut combined = Self::retry();
        for repair in repairs {
            for step in repair.steps {
                if !combined.steps.contains(&step) {
                    combined.steps.push(step);
                }
            }
            combined.retry &= repair.retry;
        }
        combined
    }

    /// The steps, then `retry` when this repair allows repeating the failed action.
    pub(crate) fn followed_by(mut self, retry: Step) -> Vec<Step> {
        if self.retry {
            self.steps.push(retry);
        }
        self.steps
    }
}

/// The repair for a failure attributed to one part of the intent.
pub(crate) fn repair(part: &IntentPart) -> Repair {
    match part {
        IntentPart::Code { plugin } => Repair::then_retry(vec![Step::Fix(FixTarget::Source {
            name: plugin.clone(),
        })]),
        IntentPart::Settings { plugin } => {
            Repair::then_retry(vec![Step::Fix(FixTarget::PluginSettings {
                name: plugin.clone(),
            })])
        }
        IntentPart::Name { name } => {
            Repair::only(vec![Step::Fix(FixTarget::Name { name: name.clone() })])
        }
        IntentPart::References { name } => {
            Repair::then_retry(vec![Step::Fix(FixTarget::References {
                name: name.clone(),
            })])
        }
        IntentPart::Use { target, .. } => Repair::then_retry(vec![Step::Fix(match target {
            enco_kernel::UseRef::Channel { name } => FixTarget::Table {
                name: format!("channel.{name}"),
            },
            enco_kernel::UseRef::Endpoint { endpoint } => FixTarget::Endpoint {
                name: endpoint.clone(),
            },
            enco_kernel::UseRef::Composer { profile } => FixTarget::Table {
                name: format!("{}.{profile}", enco_kernel::PROFILE_TABLE.name()),
            },
        })]),
        IntentPart::Table { table } => Repair::then_retry(vec![Step::Fix(FixTarget::Table {
            name: table.clone(),
        })]),
        IntentPart::Factory { plugin } => {
            Repair::only(vec![Step::AskOwner(OwnerAction::FactoryCode {
                name: plugin.clone(),
            })])
        }
        IntentPart::Plugin { .. } => Repair::then_retry(vec![CallStep::Recheck.into()]),
        IntentPart::Environment { plugin } => {
            Repair::only(vec![Step::AskOwner(OwnerAction::Environment {
                name: plugin.clone(),
            })])
        }
    }
}
