//! One template per situation: a bold "path — situation" line, then one sentence per fact.
use super::{causes::causes, code, generation, location, path};
use crate::status::{AttentionKind, Divergence, GenerationFailure, StopCause};
use enco_core::{IntentPath, PluginActivation};

pub(super) fn facts(kind: &AttentionKind<'_>) -> String {
    match kind {
        AttentionKind::New { path: target } => {
            format!("**{} — new plugin, not registered.**", path(target))
        }
        AttentionKind::Unregistered {
            path: target,
            registration,
        } => format!(
            "**{} — unregistered changes.** The files in the home differ from registration {registration}.",
            path(target)
        ),
        AttentionKind::Unbuilt {
            path: target,
            registration,
        } => format!(
            "**{} — changed source, not built yet.** A build decides whether its code differs from registration {registration}; {} registers only a real change.",
            path(target),
            code(crate::development::APPLY)
        ),
        AttentionKind::Removed {
            path: target,
            registration,
            running,
            factory,
        } => removed(target, *registration, *running, *factory),
        AttentionKind::RolledBack {
            path: target,
            running,
            registered,
            cause,
        } => rolled_back(target, *running, *registered, cause),
        AttentionKind::Diverged {
            path: target,
            divergence,
        } => diverged(target, divergence),
        AttentionKind::Stopped {
            path: target,
            cause,
        } => stopped(target, cause),
        AttentionKind::ConfigurationDiverged {
            running,
            registered,
        } => format!(
            "**{} — not running the registered configuration.** The configuration from registration {} runs; registration {} registered a newer one. Registering any edit to {} makes the whole file run, newer parts included.",
            path(&IntentPath::Configuration),
            running.registration,
            registered.registration,
            path(&IntentPath::Configuration)
        ),
        AttentionKind::FollowRejected {
            path: target,
            causes: rejected,
            running,
        } => {
            let mut out = format!(
                "**{} — host update not applied.** Registering this host's factory code was rejected: {}.",
                path(target),
                causes(rejected)
            );
            if let Some(running) = running {
                out.push_str(&format!(" The plugin keeps running {}.", generation(*running)));
            }
            out.push_str(" Every registration retries the update.");
            out
        }
        AttentionKind::UnservedUse { usage } => format!(
            "**{} — cannot be served.** Cause: {}.",
            code(&usage.user),
            causes(&usage.causes)
        ),
        AttentionKind::InvalidTable { error } => format!(
            "**{} — {} cannot be read by this host.** Cause: {}. What it configures stays stopped.",
            path(&IntentPath::Configuration),
            code(&format!("[{}]", error.table)),
            error.message
        ),
        AttentionKind::Unreadable {
            path: target,
            error,
        } => format!(
            "**{} — cannot be read.** {} {}.",
            path(target),
            location(&error.path),
            error.problem
        ),
        AttentionKind::PluginsUnreadable { error } => format!(
            "**Plugin directories cannot be read.** {} {}.",
            location(&error.path),
            error.problem
        ),
        AttentionKind::SafeMode => "**Safe mode is on.** The owner turned it on. Only recovery tools are offered, and plugins that have factory code run it.".into(),
        AttentionKind::SchedulerStopped { reason } => {
            format!("**Scheduler stopped.** Cause: {reason}. Reminders are not delivered.")
        }
        AttentionKind::SessionStopped { name, reason } => format!(
            "**Session {} stopped.** Cause: {}. Messages to it are rejected.",
            code(name),
            reason
        ),
    }
}

fn removed(
    target: &IntentPath,
    registration: enco_core::RegistrationId,
    running: bool,
    factory: bool,
) -> String {
    let mut out = format!(
        "**{} — directory removed.** Registration {registration} registered its source.",
        path(target)
    );
    if running {
        out.push_str(" The plugin still runs.");
    }
    out.push_str(if factory {
        " Registering the removal returns it to factory code."
    } else {
        " Registering the removal stops it."
    });
    out
}

fn rolled_back(
    target: &IntentPath,
    running: PluginActivation,
    registered: PluginActivation,
    cause: &GenerationFailure<'_>,
) -> String {
    let (subject, consequence, failure) = match cause {
        GenerationFailure::Code { failure } => (
            "code",
            "Registering the same source again changes nothing.",
            failure,
        ),
        GenerationFailure::Settings { failure } => (
            "settings",
            "Registering the same settings again changes nothing.",
            failure,
        ),
        GenerationFailure::Factory { failure } => (
            "factory code",
            "Only a host update changes factory code.",
            failure,
        ),
    };
    format!(
        "**{} — registered {subject} failed; running an older generation.** {}, which failed: {}. The plugin runs {} instead. {consequence}",
        path(target),
        selected(registered),
        failure.message,
        generation(running)
    )
}

fn diverged(target: &IntentPath, divergence: &Divergence<'_>) -> String {
    match divergence {
        Divergence::Name {
            running,
            registered,
            registration,
        } => format!(
            "**{} — runs as {}.** Registration {registration} named it {}; the running name comes from an earlier registration.",
            path(target),
            code(running),
            code(registered)
        ),
        Divergence::Code {
            running,
            registered,
        } => format!(
            "**{} — not running the registered code.** {}; the plugin runs {}. Registering the same source again changes nothing.",
            path(target),
            selected(*registered),
            generation(*running)
        ),
        Divergence::Settings {
            running,
            registered,
        } => format!(
            "**{} — not running the registered settings.** {} with new settings; the plugin runs {}.",
            path(target),
            selected(*registered),
            generation(*running)
        ),
        Divergence::Unregistered {
            running,
            removed_by,
        } => format!(
            "**{} — runs but is not registered.** Registration {removed_by} removed it; the plugin runs {} again.",
            path(target),
            generation(*running)
        ),
    }
}

fn stopped(target: &IntentPath, cause: &StopCause<'_>) -> String {
    let facts = match cause {
        StopCause::Failed {
            cause: GenerationFailure::Code { failure },
        } => format!(
            "Its registered code failed: {}. No older generation can run.",
            failure.message
        ),
        StopCause::Failed {
            cause: GenerationFailure::Settings { failure },
        } => format!(
            "Its registered settings failed: {}. No older generation can run.",
            failure.message
        ),
        StopCause::Failed {
            cause: GenerationFailure::Factory { failure },
        } => format!(
            "The host's factory code failed: {}. No older generation can run.",
            failure.message
        ),
        StopCause::Unavailable {
            causes: unavailable,
        } => format!(
            "This host cannot serve the selected generation: {}.",
            causes(unavailable)
        ),
        StopCause::NotRunning { registered } => format!(
            "{}, but no generation runs. Registering the same source again changes nothing.",
            selected(*registered)
        ),
    };
    format!("**{} — stopped.** {facts}", path(target))
}

/// The registered generation as the subject of a sentence.
fn selected(reference: PluginActivation) -> String {
    match reference.registration {
        Some(registration) => format!(
            "Registration {registration} registered generation {}",
            reference.generation
        ),
        None => format!(
            "The host selected generation {} with factory code",
            reference.generation
        ),
    }
}
