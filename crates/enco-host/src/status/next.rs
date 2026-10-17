use crate::development::{
    AgentReason, DevelopmentCall, HistoryPoint, HistoryQuery, Invocation, RenameArguments,
    RewindArguments,
};
use enco_core::{IntentPath, PluginName, RegistrationId};
use enco_kernel::IntentPart;
use serde::Serialize;
use std::path::PathBuf;

/// The purpose of each action is decided with the situation, never inferred by the renderer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "purpose", rename_all = "snake_case")]
pub enum Step {
    Call(CallStep),
    Fix(FixTarget),
    AskOwner(OwnerAction),
}

/// Each purpose describes exactly one effect, and carries the arguments of its call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallStep {
    Review(HistoryQuery),
    Compare(HistoryQuery),
    List(Option<RegistrationId>),
    Show(RegistrationId),
    Register,
    RegisterRemoval,
    /// Return these paths to the latest registration, dropping unregistered changes.
    Discard {
        latest: RegistrationId,
        paths: Vec<IntentPath>,
    },
    /// Return a path to the registration whose code or configuration runs.
    KeepRunning(RewindArguments),
    Restore(RewindArguments),
    Rewind(RewindArguments),
    /// Return paths to a chosen registration, overwriting their unregistered changes.
    Overwrite(RewindArguments),
    RecordName {
        name: PluginName,
        to: PluginName,
    },
    Rename {
        name: PluginName,
        to: PluginName,
    },
    Recheck,
}

/// What the Agent edits; each target is named by the facts that call for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum FixTarget {
    Source { name: PluginName },
    PluginSettings { name: PluginName },
    Endpoint { name: String },
    Table { name: String },
    Entry { path: PathBuf },
    Name { name: PluginName },
    References { name: PluginName },
    RecoveryTools,
    IgnoredEntries,
    RemoveDirectory { path: IntentPath },
    MoveDirectory { from: IntentPath, to: IntentPath },
    SeparateNames { path: IntentPath },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OwnerAction {
    Restart,
    DisableSafeMode,
    RetryDeployment,
    FactoryCode { name: PluginName },
    Environment { name: PluginName },
    HostFault,
}

impl CallStep {
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

/// At most two alternatives; each alternative is performed in order.
#[derive(Clone, Debug, Serialize)]
pub struct Steps {
    pub first: Vec<Step>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternative: Option<Vec<Step>>,
}

impl Steps {
    pub fn sequence(first: Vec<Step>) -> Self {
        Self {
            first,
            alternative: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.first.is_empty() && self.alternative.is_none()
    }
}

/// Steps that repair one failure, and whether repeating the failed action is then worthwhile.
pub struct Repair {
    pub steps: Vec<Step>,
    pub retry: bool,
}

impl Repair {
    pub fn retry() -> Self {
        Self {
            steps: Vec::new(),
            retry: true,
        }
    }

    pub fn then_retry(steps: Vec<Step>) -> Self {
        Self { steps, retry: true }
    }

    pub fn only(steps: Vec<Step>) -> Self {
        Self {
            steps,
            retry: false,
        }
    }

    /// The repair of several failures, deduplicated, retried only when each allows it.
    pub fn all(repairs: impl IntoIterator<Item = Self>) -> Self {
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

    pub fn followed_by(mut self, retry: Step) -> Vec<Step> {
        if self.retry {
            self.steps.push(retry);
        }
        self.steps
    }
}

pub fn repair(part: &IntentPart) -> Repair {
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
        IntentPart::Use { endpoint, .. } => {
            Repair::then_retry(vec![Step::Fix(FixTarget::Endpoint {
                name: endpoint.clone(),
            })])
        }
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
