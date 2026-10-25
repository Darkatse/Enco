use super::{BuildFailure, DevelopmentError, Failed, Operation};
use crate::development::{
    HistoryPoint, HistoryQuery, RewindArguments, RewindConflict, tree::WriteConflict,
};
use crate::status::{CallStep, FixTarget, OwnerAction, Repair, Step, Steps, repair};
use enco_core::{IntentPath, PluginName, RegistrationId};
use enco_kernel::RegistryError;

impl Operation {
    fn retry(&self) -> Option<Step> {
        match self {
            Self::Apply => Some(CallStep::Register.into()),
            Self::Deploy => Some(Step::AskOwner(OwnerAction::RetryDeployment)),
            Self::Startup => Some(Step::AskOwner(OwnerAction::Restart)),
            Self::Rename { name, to } => Some(
                CallStep::Rename {
                    name: name.clone(),
                    to: to.clone(),
                }
                .into(),
            ),
            Self::Rewind(arguments) => Some(CallStep::Rewind(arguments.clone()).into()),
            Self::Scaffold | Self::History | Self::Status => None,
        }
    }
}

impl Failed {
    pub fn next(&self) -> Steps {
        let repeat = |repair: Repair| match self.operation.retry() {
            Some(retry) => repair.followed_by(retry),
            None => repair.steps,
        };
        match (&self.operation, &*self.error) {
            (Operation::Rename { name, to }, DevelopmentError::Moved(_)) => Steps::sequence(vec![
                Step::Fix(FixTarget::MoveDirectory {
                    from: IntentPath::Plugin(to.clone()),
                    to: IntentPath::Plugin(name.clone()),
                }),
                CallStep::Rename {
                    name: name.clone(),
                    to: to.clone(),
                }
                .into(),
            ]),
            (
                Operation::Rewind(arguments),
                DevelopmentError::RewindRefused {
                    conflicts,
                    selected,
                },
            ) => rewind_refused(arguments, conflicts, selected),
            (Operation::Rewind(arguments), DevelopmentError::RewindCollision { path }) => {
                Steps::sequence(vec![
                    CallStep::Compare(HistoryQuery::Compare {
                        from: Some(HistoryPoint::Home),
                        to: HistoryPoint::Registration(arguments.registration),
                    })
                    .into(),
                    Step::Fix(FixTarget::SeparateNames { path: path.clone() }),
                    CallStep::Rewind(arguments.clone()).into(),
                ])
            }
            (
                _,
                DevelopmentError::AmbiguousRename {
                    missing,
                    added,
                    latest,
                },
            ) => ambiguous_rename(missing, added, *latest),
            (_, error) => Steps::sequence(repeat(error.repair())),
        }
    }
}

impl DevelopmentError {
    /// The steps that repair this failure, without repeating the operation.
    pub(crate) fn repair(&self) -> Repair {
        let host = || Repair::only(vec![Step::AskOwner(OwnerAction::HostFault)]);
        match self {
            Self::Entry(error) => Repair::then_retry(vec![Step::Fix(FixTarget::Entry {
                path: error.path.clone(),
            })]),
            Self::Build {
                failure: BuildFailure::SourceChanged,
                ..
            } => Repair::retry(),
            Self::Build {
                name,
                failure: BuildFailure::Toolchain { .. },
            } => Repair::only(vec![Step::AskOwner(OwnerAction::Environment {
                name: name.clone(),
            })]),
            Self::Build { name, .. } => {
                Repair::then_retry(vec![Step::Fix(FixTarget::Source { name: name.clone() })])
            }
            Self::Registry(error) => registry_repair(error),
            Self::Preparation(errors) => Repair::all(errors.iter().map(DevelopmentError::repair)),
            Self::Moved(source) | Self::Partial { source, .. } => source.repair(),
            Self::NameTaken { name } | Self::DirectoryExists { name } => {
                Repair::only(vec![Step::Fix(FixTarget::Name { name: name.clone() })])
            }
            Self::RewindPath { .. } => Repair::only(vec![CallStep::Recheck.into()]),
            Self::NoSource { .. } | Self::Cancelled => Repair::only(Vec::new()),
            Self::Store(_)
            | Self::Corrupt(_)
            | Self::Host(_)
            | Self::RewindContent { .. }
            | Self::WriteConflicts { .. } => host(),
            Self::RewindRefused { .. }
            | Self::RewindCollision { .. }
            | Self::AmbiguousRename { .. } => Repair::only(Vec::new()),
        }
    }
}

fn registry_repair(error: &RegistryError) -> Repair {
    match error {
        RegistryError::Rejected(causes) => {
            Repair::all(causes.iter().map(|cause| repair(&cause.part())))
        }
        RegistryError::Conflict(_) => Repair::then_retry(vec![CallStep::Recheck.into()]),
        RegistryError::Config(error) => Repair::then_retry(vec![Step::Fix(FixTarget::Table {
            name: error.table.clone(),
        })]),
        RegistryError::UnknownRegistration { .. } => {
            Repair::only(vec![CallStep::List(None).into()])
        }
        RegistryError::UnknownPlugin(_) => Repair::only(vec![CallStep::Recheck.into()]),
        RegistryError::NoRollbackTarget => Repair::only(Vec::new()),
        RegistryError::NoRegistrations
        | RegistryError::Intent(_)
        | RegistryError::Store(_)
        | RegistryError::FactoryIdentity(_)
        | RegistryError::EmptyRequest
        | RegistryError::NotFactory(_)
        | RegistryError::FactoryRemoval(_)
        | RegistryError::UnknownGeneration(_)
        | RegistryError::UnknownProfile(_)
        | RegistryError::Bootstrap(_)
        | RegistryError::TableOwner(_) => {
            Repair::only(vec![Step::AskOwner(OwnerAction::HostFault)])
        }
    }
}

/// Unregistered changes are kept unless the Agent overwrites them; registering them first makes
/// the same rewind succeed, because their paths then changed after the target. Overwriting writes
/// every path the refused rewind selected, not only the blocked ones.
fn rewind_refused(
    arguments: &RewindArguments,
    conflicts: &[RewindConflict],
    selected: &[IntentPath],
) -> Steps {
    let blocked = |conflict: &WriteConflict| match conflict {
        WriteConflict::Unregistered { .. } => false,
        WriteConflict::Occupied { .. }
        | WriteConflict::Ignored { .. }
        | WriteConflict::IgnoreRulesChanged { .. } => true,
    };
    let entries = conflicts.iter().flat_map(|refused| &refused.conflicts);
    let clear = entries
        .clone()
        .any(blocked)
        .then_some(Step::Fix(FixTarget::IgnoredEntries));
    let unregistered = entries.clone().any(|conflict| !blocked(conflict));
    let rewind = CallStep::Rewind(arguments.clone()).into();
    if !unregistered {
        return Steps::sequence(clear.into_iter().chain([rewind]).collect());
    }
    Steps {
        first: clear
            .clone()
            .into_iter()
            .chain([CallStep::Register.into(), rewind])
            .collect(),
        alternative: Some(
            clear
                .into_iter()
                .chain([CallStep::Overwrite(RewindArguments {
                    registration: arguments.registration,
                    paths: Some(selected.to_vec()),
                })
                .into()])
                .collect(),
        ),
    }
}

/// A moved directory is a rename. Otherwise the new plugin registers while the old directory is
/// back, and the old plugin is removed in a registration of its own.
fn ambiguous_rename(names: &[PluginName], added: &[PluginName], latest: RegistrationId) -> Steps {
    let missing: Vec<_> = names.iter().cloned().map(IntentPath::Plugin).collect();
    let mut restore = vec![
        CallStep::Restore(RewindArguments {
            registration: latest,
            paths: Some(missing.clone()),
        })
        .into(),
        CallStep::Register.into(),
    ];
    restore.extend(
        missing
            .into_iter()
            .map(|path| Step::Fix(FixTarget::RemoveDirectory { path })),
    );
    restore.push(CallStep::RegisterRemoval.into());
    match (names, added) {
        ([name], [to]) => Steps {
            first: vec![
                Step::Fix(FixTarget::MoveDirectory {
                    from: IntentPath::Plugin(to.clone()),
                    to: IntentPath::Plugin(name.clone()),
                }),
                CallStep::Rename {
                    name: name.clone(),
                    to: to.clone(),
                }
                .into(),
            ],
            alternative: Some(restore),
        },
        _ => Steps::sequence(restore),
    }
}
