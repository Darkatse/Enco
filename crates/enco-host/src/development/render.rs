//! Operational documents render complete facts and purpose-bearing actions; nothing here decides.
mod actions;
mod causes;
mod errors;
mod history;
mod situations;
mod status;
use super::{HomeComparison, Invocation, SettingsComparison};
use crate::{
    clock::LOCAL_TIME,
    status::{CallStep, FixTarget, OwnerAction, Step, Steps},
};
pub use actions::registration_document;
pub(super) use causes::causes;
use enco_core::{
    DateTime, GenerationStatus, Initiator, IntentPath, PluginActivation, SessionRef, Tz, Utc,
};
pub(super) use errors::failure;

fn time(at: DateTime<Utc>, timezone: Tz) -> String {
    at.with_timezone(&timezone).format(LOCAL_TIME).to_string()
}

fn caller(initiator: &Initiator<SessionRef>) -> String {
    match initiator {
        Initiator::Owner { .. } => "by the owner".into(),
        Initiator::Host => "by the host".into(),
        Initiator::Agent { session, .. } => format!("by the Agent in {}", code(&session.name)),
    }
}

/// A reason is quoted data, written as a JSON string.
fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// The shortest run of backticks longer than any run inside `text`, at least `minimum` long.
fn fence(text: &str, minimum: usize) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    "`".repeat((longest + 1).max(minimum))
}

fn code(text: &str) -> String {
    let fence = fence(text, 1);
    if fence.len() == 1 {
        format!("{fence}{text}{fence}")
    } else {
        format!("{fence} {text} {fence}")
    }
}
fn path(value: &IntentPath) -> String {
    code(&value.to_string())
}
fn location(value: &std::path::Path) -> String {
    code(&value.display().to_string())
}
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', "<br>")
}
fn generation_state(state: GenerationStatus) -> &'static str {
    match state {
        GenerationStatus::Trial => "trial",
        GenerationStatus::Healthy => "healthy",
        GenerationStatus::Failed => "failed",
    }
}
/// A generation as the object of a sentence, never its start.
fn generation(reference: PluginActivation) -> String {
    match reference.registration {
        Some(registration) => format!(
            "generation {} from registration {registration}",
            reference.generation
        ),
        None => format!("generation {} with factory code", reference.generation),
    }
}
fn comparison(comparison: &HomeComparison) -> &'static str {
    match comparison {
        HomeComparison::Same => "matches",
        HomeComparison::TextOnly => "text-only changes",
        HomeComparison::Changed => "changed",
        HomeComparison::Unbuilt => "changed · not built",
        HomeComparison::Removed => "removed",
        HomeComparison::Absent => "no directory",
        HomeComparison::Unreadable(_) => "cannot read",
    }
}
fn settings(comparison: SettingsComparison) -> Option<&'static str> {
    match comparison {
        SettingsComparison::Same => None,
        SettingsComparison::Changed => Some("settings changed"),
        SettingsComparison::Unknown => Some("settings cannot be compared"),
    }
}

#[expect(
    clippy::expect_used,
    reason = "tool arguments are structs of strings, numbers and sequences with string keys, which JSON always represents"
)]
fn invocation(call: &Invocation) -> String {
    let arguments = serde_json::to_string(&call.arguments).expect("tool arguments serialize");
    code(&format!("{} {arguments}", call.tool))
}

fn step(step: &Step) -> String {
    match step {
        Step::Call(call) => {
            let purpose = match call {
                CallStep::Review(_) => "review with",
                CallStep::Compare(_) => "compare with",
                CallStep::List(_) => "list registrations with",
                CallStep::Show(_) => "see one registration in full with",
                CallStep::Register => "register with",
                CallStep::RegisterRemoval => "register the removal with",
                CallStep::Discard { .. } => "discard them with",
                CallStep::KeepRunning(_) => "keep what runs with",
                CallStep::Restore(_) => "bring it back with",
                CallStep::Rewind(_) => "rewind with",
                CallStep::Overwrite(_) => "overwrite them with",
                CallStep::RecordName { .. } => "record the running name with",
                CallStep::Rename { .. } => "rename it with",
                CallStep::Recheck => "check again with",
            };
            format!("{purpose} {}", invocation(&call.invocation()))
        }
        Step::Fix(target) => fix(target),
        Step::AskOwner(action) => ask(action),
    }
}

fn fix(target: &FixTarget) -> String {
    let table = |name: String| {
        format!(
            "fix {} in {}",
            code(&format!("[{name}]")),
            path(&IntentPath::Configuration)
        )
    };
    match target {
        FixTarget::Source { name } => format!(
            "fix the source in {}",
            path(&IntentPath::Plugin(name.clone()))
        ),
        FixTarget::PluginSettings { name } => table(format!(
            "{}.{name}",
            enco_kernel::PLUGIN_SETTINGS_TABLE.name()
        )),
        FixTarget::Endpoint { name } => {
            table(format!("{}.{name}", enco_kernel::ENDPOINT_TABLE.name()))
        }
        FixTarget::Table { name } => table(name.clone()),
        FixTarget::Entry { path } => format!("fix {}", location(path)),
        FixTarget::Name { name } => format!("choose a name other than {}", code(name)),
        FixTarget::References { name } => format!(
            "remove the references to {} from {}",
            code(name),
            path(&IntentPath::Configuration)
        ),
        FixTarget::RecoveryTools => "repair with the recovery tools".into(),
        FixTarget::IgnoredEntries => {
            "move the listed ignored entries out of the affected paths".into()
        }
        FixTarget::RemoveDirectory { path: target } => format!("delete {}", path(target)),
        FixTarget::MoveDirectory { from, to } => {
            format!("move {} back to {}", path(from), path(to))
        }
        FixTarget::SeparateNames { path: target } => format!(
            "rename the plugin now at {} before restoring the earlier one",
            path(target)
        ),
    }
}

fn ask(action: &OwnerAction) -> String {
    match action {
        OwnerAction::Restart => "ask the owner to restart Enco".into(),
        OwnerAction::DisableSafeMode => format!(
            "ask the owner to turn safe mode off with {}",
            code("enco safe-mode off")
        ),
        OwnerAction::RetryDeployment => "ask the owner to deploy the component again".into(),
        OwnerAction::FactoryCode { name } => format!(
            "ask the owner to repair the host's factory code for {}",
            code(name)
        ),
        OwnerAction::Environment { name } => format!(
            "ask the owner to provide what {} needs from its environment",
            code(name)
        ),
        OwnerAction::HostFault => "ask the owner to look into this host failure".into(),
    }
}

fn sequence(steps: &[Step]) -> String {
    steps.iter().map(step).collect::<Vec<_>>().join(", then ")
}

fn next(steps: &Steps) -> String {
    if steps.is_empty() {
        return String::new();
    }
    let mut result = format!("Next: {}", sequence(&steps.first));
    if let Some(alternative) = &steps.alternative {
        result.push_str(&format!("; or {}", sequence(alternative)));
    }
    result.push('.');
    result
}
