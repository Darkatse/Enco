use super::{
    Attention, AttentionKind, CallStep, FixTarget, OwnerAction, Repair, Status, Step, Steps, repair,
};
use crate::development::{
    HistoryPoint, HistoryQuery, PluginUnit, RewindArguments, WorkspaceComparison,
};
use enco_core::{IntentPath, PluginActivation, PluginName, RegistrationId};
use enco_kernel::{Activation, Cause, Running};

mod runtime;
use runtime::runtime;

pub(super) fn attention(status: &Status) -> Vec<Attention<'_>> {
    let facts = &status.development;
    let mut out = Vec::new();
    workspace(
        IntentPath::Configuration,
        &facts.configuration.workspace,
        facts.registration,
        &mut out,
    );
    if let Some(error) = &facts.plugins_unreadable {
        out.push(Attention {
            kind: AttentionKind::PluginsUnreadable { error },
            next: fix_then_recheck(&error.path),
        });
    }
    for (name, plugin) in &facts.plugins {
        plugin_workspace(name, plugin, facts.registration, &mut out);
        runtime(name, plugin, &mut out);
        follow(name, plugin, &mut out);
    }
    configuration(&facts.configuration, &mut out);
    execution(&status.node.execution, &mut out);
    out
}

fn plugin_workspace<'a>(
    name: &PluginName,
    plugin: &'a PluginUnit,
    latest: RegistrationId,
    out: &mut Vec<Attention<'a>>,
) {
    let Some(directory) = &plugin.workspace else {
        return;
    };
    let path = IntentPath::Plugin(name.clone());
    match (&directory.code, &plugin.registered) {
        (WorkspaceComparison::Removed, Some(registered)) => {
            if let Some(code) = &registered.plugin.code {
                out.push(Attention {
                    kind: AttentionKind::Removed {
                        path: path.clone(),
                        registration: code.registration,
                        factory: registered.plugin.factory.is_some(),
                        running: plugin.running.as_ref().is_some_and(|status| {
                            matches!(status.running, Running::Serving { .. })
                        }),
                    },
                    next: Steps {
                        first: vec![CallStep::RegisterRemoval.into()],
                        alternative: Some(vec![
                            CallStep::Restore(RewindArguments::path(code.registration, path))
                                .into(),
                        ]),
                    },
                });
            }
        }
        (WorkspaceComparison::Changed | WorkspaceComparison::Unbuilt, None) => {
            out.push(Attention {
                kind: AttentionKind::New { path },
                next: Steps::sequence(vec![CallStep::Register.into()]),
            })
        }
        (comparison, _) => workspace(path, comparison, latest, out),
    }
}

fn workspace<'a>(
    path: IntentPath,
    comparison: &'a WorkspaceComparison,
    latest: RegistrationId,
    out: &mut Vec<Attention<'a>>,
) {
    let review = || -> Step {
        CallStep::Review(HistoryQuery::Compare {
            from: None,
            to: HistoryPoint::Workspace,
        })
        .into()
    };
    let (kind, next) = match comparison {
        WorkspaceComparison::Changed => (
            AttentionKind::Unregistered {
                path: path.clone(),
                registration: latest,
            },
            Steps {
                first: vec![review(), CallStep::Register.into()],
                alternative: Some(vec![
                    CallStep::Discard {
                        latest,
                        paths: vec![path],
                    }
                    .into(),
                ]),
            },
        ),
        WorkspaceComparison::Unbuilt => (
            AttentionKind::Unbuilt {
                path,
                registration: latest,
            },
            Steps::sequence(vec![review(), CallStep::Register.into()]),
        ),
        WorkspaceComparison::Unreadable(error) => (
            AttentionKind::Unreadable { path, error },
            fix_then_recheck(&error.path),
        ),
        WorkspaceComparison::Same
        | WorkspaceComparison::TextOnly
        | WorkspaceComparison::Removed
        | WorkspaceComparison::Absent => return,
    };
    out.push(Attention { kind, next });
}

fn follow<'a>(name: &PluginName, plugin: &'a PluginUnit, out: &mut Vec<Attention<'a>>) {
    let Some(causes) = plugin
        .registered
        .as_ref()
        .and_then(|unit| unit.follow_rejection.as_deref())
    else {
        return;
    };
    out.push(Attention {
        kind: AttentionKind::FollowRejected {
            path: IntentPath::Plugin(name.clone()),
            causes,
            running: plugin
                .running
                .as_ref()
                .and_then(|status| match &status.running {
                    Running::Serving { activation, .. } => Some(reference(activation)),
                    Running::Unavailable { .. } | Running::Stopped => None,
                }),
        },
        next: repairs(causes),
    });
}

fn configuration<'a>(
    config: &'a crate::development::ConfigurationUnit,
    out: &mut Vec<Attention<'a>>,
) {
    if config.running != config.registered {
        out.push(Attention {
            kind: AttentionKind::ConfigurationDiverged {
                running: config.running,
                registered: config.registered,
            },
            next: Steps {
                first: vec![compare(
                    Some(config.running.registration),
                    config.registered.registration,
                )],
                alternative: Some(vec![
                    CallStep::KeepRunning(RewindArguments::path(
                        config.running.registration,
                        IntentPath::Configuration,
                    ))
                    .into(),
                    CallStep::Register.into(),
                ]),
            },
        });
    }
    for error in &config.invalid {
        out.push(Attention {
            kind: AttentionKind::InvalidTable { error },
            next: Steps::sequence(vec![
                Step::Fix(FixTarget::Table {
                    name: error.table.clone(),
                }),
                CallStep::Register.into(),
            ]),
        });
    }
    // A use of a plugin that cannot run is reported once, with the plugin.
    for usage in &config.unavailable_uses {
        if usage
            .causes
            .iter()
            .all(|cause| matches!(cause, Cause::PluginUnavailable { .. }))
        {
            continue;
        }
        out.push(Attention {
            kind: AttentionKind::UnservedUse { usage },
            next: repairs(&usage.causes),
        });
    }
}

fn execution<'a>(node: &'a enco_kernel::Status, out: &mut Vec<Attention<'a>>) {
    if node.safe_mode {
        out.push(Attention {
            kind: AttentionKind::SafeMode,
            next: Steps::sequence(vec![
                Step::Fix(FixTarget::RecoveryTools),
                Step::AskOwner(OwnerAction::DisableSafeMode),
            ]),
        });
    }
    if let Some(reason) = &node.scheduler_stopped {
        out.push(Attention {
            kind: AttentionKind::SchedulerStopped { reason },
            next: Steps::sequence(vec![Step::AskOwner(OwnerAction::Restart)]),
        });
    }
    for session in &node.sessions {
        if let Some(reason) = &session.stopped {
            out.push(Attention {
                kind: AttentionKind::SessionStopped {
                    name: &session.session.name,
                    reason,
                },
                next: Steps::sequence(vec![Step::AskOwner(OwnerAction::Restart)]),
            });
        }
    }
}

pub(super) fn repairs(causes: &[Cause]) -> Steps {
    Steps::sequence(
        Repair::all(causes.iter().map(|cause| repair(&cause.part())))
            .followed_by(CallStep::Register.into()),
    )
}

pub(super) fn fix_source(name: &PluginName) -> Vec<Step> {
    vec![
        Step::Fix(FixTarget::Source { name: name.clone() }),
        CallStep::Register.into(),
    ]
}

fn fix_then_recheck(path: &std::path::Path) -> Steps {
    Steps::sequence(vec![
        Step::Fix(FixTarget::Entry {
            path: path.to_owned(),
        }),
        CallStep::Recheck.into(),
    ])
}

pub(super) fn reference(activation: &Activation) -> PluginActivation {
    PluginActivation {
        generation: activation.generation.id,
        registration: activation.registration,
    }
}

pub(super) fn compare(from: Option<RegistrationId>, to: RegistrationId) -> Step {
    CallStep::Compare(HistoryQuery::Compare {
        from: from.map(HistoryPoint::Registration),
        to: HistoryPoint::Registration(to),
    })
    .into()
}
