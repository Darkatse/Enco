//! A failure document says what the operation left behind, what failed, and what to do next.
use super::{causes::causes, code, history, location, next, path};
use crate::development::{
    BuildFailure, DevelopmentError, Failed, Operation, RewindConflict, ToolchainProblem,
    build::TARGET, tree::WriteConflict,
};
use enco_core::IntentPath;
use enco_kernel::RegistryError;
use std::path::PathBuf;

pub(in crate::development) fn failure(failed: &Failed) -> String {
    let mut out = left_behind(&failed.operation, &failed.error);
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(&facts(&failed.error));
    let next = next(&failed.next());
    if !next.is_empty() {
        out.push('\n');
        out.push_str(&next);
    }
    out
}

/// What the operation changed before it failed; reading changes nothing.
fn left_behind(operation: &Operation, error: &DevelopmentError) -> String {
    let completed = |written: &[PathBuf]| {
        let mut out = String::from("Completed file operations:");
        for path in written {
            out.push_str(&format!("\n- {}", location(path)));
        }
        out.push('\n');
        out
    };
    match (operation, error) {
        (Operation::Rename { name, to }, DevelopmentError::Moved(_)) => format!(
            "Not registered. The source moved from {} to {}.",
            path(&IntentPath::Plugin(name.clone())),
            path(&IntentPath::Plugin(to.clone()))
        ),
        (Operation::Rewind(_) | Operation::Scaffold, DevelopmentError::Partial { written, .. })
            if !written.is_empty() =>
        {
            format!("Stopped before finishing. {}", completed(written))
        }
        (Operation::Apply | Operation::Startup | Operation::Rename { .. }, _) => {
            "Not registered.".into()
        }
        (Operation::Rewind(_) | Operation::Scaffold, _) => "Nothing was written.".into(),
        (Operation::History | Operation::Status, _) => String::new(),
    }
}

/// The facts of one failure, without steps; nested failures use the same sentences.
fn facts(error: &DevelopmentError) -> String {
    match error {
        DevelopmentError::Entry(error) => format!("{} {}.", location(&error.path), error.problem),
        DevelopmentError::Build { name, failure } => {
            build(&IntentPath::Plugin(name.clone()), failure)
        }
        DevelopmentError::Registry(error) => registry(error),
        DevelopmentError::Store(error) => format!("The host's storage failed: {error}."),
        DevelopmentError::Corrupt(reason) => format!("Stored content is corrupt: {reason}."),
        DevelopmentError::Host(reason) => format!("The host failed: {reason}."),
        DevelopmentError::NameTaken { name } => {
            format!("{} is already registered or reserved.", code(name))
        }
        DevelopmentError::DirectoryExists { name } => {
            format!(
                "{} already exists.",
                path(&IntentPath::Plugin(name.clone()))
            )
        }
        DevelopmentError::NoSource { name } => format!(
            "{} has no registered source directory; only plugins with source can be renamed.",
            code(name)
        ),
        DevelopmentError::AmbiguousRename { missing, added, .. } => format!(
            "Registered {} {} missing while {} {} new, so registering cannot tell a rename from a removal and a new plugin.",
            paths(missing),
            if missing.len() == 1 { "is" } else { "are" },
            paths(added),
            if added.len() == 1 { "is" } else { "are" },
        ),
        DevelopmentError::Preparation(errors) => {
            let failures: String = errors
                .iter()
                .map(|error| format!("\n- {}", facts(error)))
                .collect();
            format!("These paths cannot be prepared:{failures}")
        }
        DevelopmentError::Moved(source) | DevelopmentError::Partial { source, .. } => facts(source),
        DevelopmentError::RewindRefused { conflicts, .. } => refused(conflicts),
        DevelopmentError::RewindPath { path: target } => format!(
            "{} cannot be rewound: neither registration has a plugin under this name.",
            path(target)
        ),
        DevelopmentError::RewindCollision { path: target } => format!(
            "Two different plugins would be written to {}.",
            path(target)
        ),
        DevelopmentError::RewindContent {
            path: target,
            unavailable,
        } => format!(
            "{} cannot be read from {}.",
            path(target),
            history::unavailable(unavailable)
        ),
        DevelopmentError::WriteConflicts { path, conflicts } => format!(
            "{} holds entries the host does not own:{}",
            location(path),
            list(conflicts)
        ),
        DevelopmentError::Cancelled => "Cancelled.".into(),
    }
}

fn build(target: &IntentPath, failure: &BuildFailure) -> String {
    match failure {
        BuildFailure::Cargo {
            stage,
            diagnostics,
            log,
        } => format!(
            "{}: {} failed.\n{}\nFull log: {}\nLogs are retained until the build directory is cleaned.",
            path(target),
            code(&format!("cargo {stage}")),
            diagnostics.join("\n"),
            location(log)
        ),
        BuildFailure::NoArtifact(log) => format!(
            "{}: Cargo produced no component. Full log: {}\nLogs are retained until the build directory is cleaned.",
            path(target),
            location(log)
        ),
        BuildFailure::EmptySettings(error) => {
            format!("{} cannot load with empty settings: {error}.", path(target))
        }
        BuildFailure::SourceChanged => format!("{} changed while it was built.", path(target)),
        BuildFailure::Toolchain { channel, problem } => {
            let install = code(&format!(
                "rustup toolchain install {channel} --profile minimal --component clippy --target {TARGET}"
            ));
            match problem {
                ToolchainProblem::Missing => format!(
                    "{} builds with Rust {channel}, which is not installed. Install it with {install}.",
                    path(target)
                ),
                ToolchainProblem::Target => format!(
                    "{} builds with Rust {channel}, which lacks the {} target. Install it with {install}.",
                    path(target),
                    code(TARGET)
                ),
                ToolchainProblem::Release { found } => format!(
                    "{} builds with Rust {channel}, but {} is release {found}, so it does not go through rustup. Point {} or {} at rustup's {}.",
                    path(target),
                    code("rustc"),
                    code("RUSTC"),
                    code("PATH"),
                    code("rustc")
                ),
            }
        }
    }
}

fn registry(error: &RegistryError) -> String {
    match error {
        RegistryError::Rejected(rejected) => format!("Rejected: {}.", causes(rejected)),
        RegistryError::Conflict(_) => "Another change was registered first.".into(),
        RegistryError::UnknownRegistration {
            registration,
            latest,
        } => format!(
            "Registration {registration} does not exist; the latest is registration {latest}."
        ),
        RegistryError::UnknownPlugin(name) => format!("{} is not registered.", code(name)),
        RegistryError::NoRollbackTarget => "No registration can be rolled back.".into(),
        RegistryError::Config(error) => {
            format!(
                "{}: {}.",
                code(&format!("[{}]", error.table)),
                error.message
            )
        }
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
        | RegistryError::TableOwner(_) => format!("The plugin registry failed: {error}."),
    }
}

fn refused(blocked: &[RewindConflict]) -> String {
    let entries: String = blocked
        .iter()
        .map(|blocked| list(&blocked.conflicts))
        .collect();
    format!("These entries block the rewind:{entries}")
}

fn list(conflicts: &[WriteConflict]) -> String {
    conflicts
        .iter()
        .map(|conflict| {
            let (entry, reason) = match conflict {
                WriteConflict::Unregistered { path } => (path, "has unregistered changes"),
                WriteConflict::Occupied { path } => {
                    (path, "is an ignored entry where a registered file belongs")
                }
                WriteConflict::Ignored { path } => (
                    path,
                    "is an ignored entry that keeps the directory from being removed",
                ),
                WriteConflict::IgnoreRulesChanged { path } => (
                    path,
                    "is ignored now and could become source under the restored ignore rules",
                ),
            };
            format!("\n- {} {reason}.", location(entry))
        })
        .collect()
}

fn paths(names: &[enco_core::PluginName]) -> String {
    names
        .iter()
        .map(|name| path(&IntentPath::Plugin(name.clone())))
        .collect::<Vec<_>>()
        .join(", ")
}
