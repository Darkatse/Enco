//! One template per situation: a bold "path — situation" line, then one sentence per fact.
use super::{causes::causes, code, location, path};
use crate::status::{AttentionKind, Divergence, GenerationFailure, StopCause};
use enco_core::{IntentPath, PluginActivation, PluginCode};

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
        } => rolled_back(target, running, registered, cause),
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
            running,
            registered,
            path(&IntentPath::Configuration)
        ),
        AttentionKind::FollowRejected {
            path: target,
            causes: rejected,
            running,
        } => follow_rejected(target, rejected, running.as_ref()),
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
        AttentionKind::ProfilesUnreadable { error } => format!(
            "**Profile instructions cannot be listed.** {} {}.", location(&error.path), error.problem
        ),
        AttentionKind::UnmatchedProfileInstructions { path } => format!(
            "**{} — no matching profile.** The running configuration has no profile for this file, so it is not read.",
            location(path)
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

fn follow_rejected(
    target: &IntentPath,
    rejected: &[enco_kernel::Cause],
    running: Option<&PluginActivation>,
) -> String {
    let mut out = format!(
        "**{} — host update not applied.** Registering this host's factory code was rejected: {}.",
        path(target),
        causes(rejected)
    );
    if let Some(running) = running {
        out.push_str(&format!(" It keeps running {}.", running.code()));
    }
    out.push_str(" Every registration retries the update.");
    out
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
    running: &PluginActivation,
    registered: &PluginActivation,
    cause: &GenerationFailure<'_>,
) -> String {
    let consequence = match cause {
        GenerationFailure::Code { .. } => "Registering the same source again changes nothing.",
        GenerationFailure::Settings { .. } => {
            "Registering the same settings again changes nothing."
        }
        GenerationFailure::Factory { .. } => "Only a host update changes factory code.",
    };
    format!(
        "**{} — rolled back to {}.** {} failed: {}. {consequence}",
        path(target),
        running.code(),
        failed(registered, cause),
        cause.failure().message,
    )
}

/// What a failed generation brought, as the subject of a sentence.
fn failed(registered: &PluginActivation, cause: &GenerationFailure<'_>) -> String {
    let registration = registered.registration().map_or_else(
        || "The registered code".into(),
        |id| format!("Registration {id}"),
    );
    match cause {
        GenerationFailure::Code { .. } => format!("{registration}'s code"),
        GenerationFailure::Settings { .. } => format!("{registration}'s settings"),
        GenerationFailure::Factory { .. } => format!("{registration}'s factory code"),
    }
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
            "**{} — runs {}, not the registered {}.** Registering the same source again changes nothing.",
            path(target),
            running.code(),
            short(registered)
        ),
        Divergence::Settings {
            running,
            registered,
        } => format!(
            "**{} — runs {}, not the registered {}.** Their settings differ.",
            path(target),
            running.code(),
            short(registered)
        ),
        Divergence::Unregistered {
            running,
            removed_by,
        } => format!(
            "**{} — runs but is not registered.** Registration {removed_by} removed it; it runs {} again.",
            path(target),
            running.code()
        ),
    }
}

/// Registered code after "the registered": its number, or factory code.
fn short(registered: &PluginActivation) -> String {
    match registered.code() {
        PluginCode::Generation { registration } => registration.to_string(),
        code @ PluginCode::Factory { .. } => code.to_string(),
    }
}

fn stopped(target: &IntentPath, cause: &StopCause<'_>) -> String {
    let facts = match cause {
        StopCause::Failed { registered, cause } => format!(
            "{} failed: {}. Nothing earlier can run.",
            failed(registered, cause),
            cause.failure().message
        ),
        StopCause::Unavailable {
            selected,
            causes: unavailable,
        } => format!(
            "This host cannot run {}: {}.",
            selected.code(),
            causes(unavailable)
        ),
        StopCause::NotRunning { registered } => match registered.registration() {
            Some(registration) => format!(
                "Registration {registration} registered it, but nothing runs. Registering the same source again changes nothing."
            ),
            None => "The host selected factory code, but nothing runs.".into(),
        },
    };
    format!("**{} — stopped.** {facts}", path(target))
}
