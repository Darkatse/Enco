//! What runs, against what is registered: one situation per layer that differs.
use super::{compare, fix_source, reference, repairs};
use crate::development::{PluginUnit, RewindArguments};
use crate::status::{
    Attention, AttentionKind, CallStep, Divergence, FixTarget, GenerationFailure, OwnerAction,
    Step, Steps, StopCause,
};
use enco_core::{IntentPath, Origin, PluginActivation, PluginName, RegistrationId};
use enco_kernel::{Activation, CodeSource, Difference, RegisteredPlugin, Running, Standing};

pub(super) fn runtime<'a>(
    name: &'a PluginName,
    plugin: &'a PluginUnit,
    out: &mut Vec<Attention<'a>>,
) {
    let Some(status) = &plugin.running else {
        return;
    };
    let registered = plugin.registered.as_ref().map(|unit| &unit.plugin);
    match (&status.running, &status.standing, registered) {
        (Running::Unavailable { causes, .. }, _, _) => out.push(Attention {
            kind: AttentionKind::Stopped {
                path: IntentPath::Plugin(name.clone()),
                cause: StopCause::Unavailable { causes },
            },
            next: repairs(causes),
        }),
        (Running::Serving { activation, .. }, Standing::Removed { by }, _) => {
            out.push(unregistered(name, activation, *by));
        }
        (
            Running::Serving { activation, .. },
            Standing::Registered { differences },
            Some(registered),
        ) => match rolled_back(name, activation, registered) {
            Some(attention) => out.push(attention),
            None => diverged(name, activation, differences, registered, out),
        },
        (Running::Stopped, Standing::Registered { .. }, Some(registered)) => {
            out.push(stopped(name, &registered.activation));
        }
        // A registered standing always comes with the registered layer, and a removed plugin
        // that does not run is not listed.
        (Running::Serving { .. } | Running::Stopped, Standing::Registered { .. }, None)
        | (Running::Stopped, Standing::Removed { .. }, _) => {}
    }
}

fn rolled_back<'a>(
    name: &PluginName,
    running: &'a Activation,
    plugin: &'a RegisteredPlugin,
) -> Option<Attention<'a>> {
    let registered = &plugin.activation;
    if registered.generation.id == running.generation.id {
        return None;
    }
    let cause = failure(registered)?;
    let (current, selected) = (reference(running), reference(registered));
    let next = match cause {
        GenerationFailure::Code { .. } => Steps {
            first: fix_source(name),
            alternative: keep_running(name, running, plugin),
        },
        GenerationFailure::Factory { .. } => Steps {
            first: ask_about_factory(name),
            alternative: keep_running(name, running, plugin),
        },
        GenerationFailure::Settings { .. } => settings_steps(name, current, selected),
    };
    Some(Attention {
        kind: AttentionKind::RolledBack {
            path: IntentPath::Plugin(name.clone()),
            running: current,
            registered: selected,
            cause,
        },
        next,
    })
}

fn diverged<'a>(
    name: &'a PluginName,
    running: &'a Activation,
    differences: &'a [Difference],
    registered: &'a RegisteredPlugin,
    out: &mut Vec<Attention<'a>>,
) {
    let current = reference(running);
    let selected = reference(&registered.activation);
    for difference in differences {
        let (divergence, next) = match difference {
            Difference::Name { running: actual } => {
                // Only a registered name can differ from the running one; the host's own
                // unregistered factory names never do.
                let Some(registration) = registered.named_by else {
                    continue;
                };
                (
                    Divergence::Name {
                        running: actual,
                        registered: name,
                        registration,
                    },
                    Steps::sequence(vec![
                        CallStep::RecordName {
                            name: name.clone(),
                            to: actual.clone(),
                        }
                        .into(),
                    ]),
                )
            }
            Difference::Code => (
                Divergence::Code {
                    running: current,
                    registered: selected,
                },
                match keep_running(name, running, registered) {
                    Some(keep) => Steps {
                        first: keep,
                        alternative: Some(fix_source(name)),
                    },
                    None => Steps::sequence(fix_source(name)),
                },
            ),
            Difference::Settings => (
                Divergence::Settings {
                    running: current,
                    registered: selected,
                },
                settings_steps(name, current, selected),
            ),
        };
        out.push(Attention {
            kind: AttentionKind::Diverged {
                path: IntentPath::Plugin(name.clone()),
                divergence,
            },
            next,
        });
    }
}

fn unregistered<'a>(
    name: &PluginName,
    running: &Activation,
    removed_by: RegistrationId,
) -> Attention<'a> {
    let current = reference(running);
    Attention {
        kind: AttentionKind::Diverged {
            path: IntentPath::Plugin(name.clone()),
            divergence: Divergence::Unregistered {
                running: current,
                removed_by,
            },
        },
        next: Steps::sequence(vec![compare(current.registration, removed_by)]),
    }
}

fn stopped<'a>(name: &PluginName, registered: &'a Activation) -> Attention<'a> {
    let path = IntentPath::Plugin(name.clone());
    let (cause, next) = match failure(registered) {
        Some(cause) => {
            let steps = match cause {
                GenerationFailure::Code { .. } => fix_source(name),
                GenerationFailure::Settings { .. } => vec![
                    Step::Fix(FixTarget::PluginSettings { name: name.clone() }),
                    CallStep::Register.into(),
                ],
                GenerationFailure::Factory { .. } => ask_about_factory(name),
            };
            (StopCause::Failed { cause }, Steps::sequence(steps))
        }
        None => (
            StopCause::NotRunning {
                registered: reference(registered),
            },
            Steps::sequence(fix_source(name)),
        ),
    };
    Attention {
        kind: AttentionKind::Stopped { path, cause },
        next,
    }
}

/// Code shipped with the host changes only with the host.
fn ask_about_factory(name: &PluginName) -> Vec<Step> {
    vec![Step::AskOwner(OwnerAction::FactoryCode {
        name: name.clone(),
    })]
}

/// Steps that keep the running code: rewind the plugin to the registration it came from, then
/// register.
///
/// Keeping what runs needs the running code to come from a registered source other than the
/// one registered now; rewinding to the registered source would register nothing.
fn keep_running(
    name: &PluginName,
    running: &Activation,
    registered: &RegisteredPlugin,
) -> Option<Vec<Step>> {
    let CodeSource::Directory { registration, .. } = running.code.source else {
        return None;
    };
    if registered
        .code
        .as_ref()
        .is_some_and(|code| code.registration == registration)
    {
        return None;
    }
    Some(vec![
        CallStep::KeepRunning(RewindArguments::path(
            registration,
            IntentPath::Plugin(name.clone()),
        ))
        .into(),
        CallStep::Register.into(),
    ])
}

/// Settings live in one table of `config.toml`; rewinding the whole file would change others.
fn settings_steps(
    name: &PluginName,
    running: PluginActivation,
    registered: PluginActivation,
) -> Steps {
    let mut steps = Vec::new();
    if let Some(to) = registered.registration {
        steps.push(compare(running.registration, to));
    }
    steps.extend([
        Step::Fix(FixTarget::PluginSettings { name: name.clone() }),
        CallStep::Register.into(),
    ]);
    Steps::sequence(steps)
}

/// A recorded failure, attributed to what the failed generation introduced.
fn failure(activation: &Activation) -> Option<GenerationFailure<'_>> {
    let failure = activation.generation.failure.as_ref()?;
    Some(
        match (activation.generation.origin, activation.code.source) {
            (Origin::Settings { .. }, _) => GenerationFailure::Settings { failure },
            (
                Origin::Code { .. } | Origin::Artifact { .. } | Origin::Factory,
                CodeSource::Factory,
            ) => GenerationFailure::Factory { failure },
            (
                Origin::Code { .. } | Origin::Artifact { .. } | Origin::Factory,
                CodeSource::Directory { .. } | CodeSource::Artifact,
            ) => GenerationFailure::Code { failure },
        },
    )
}
