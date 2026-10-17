//! What failed, what the failed operation left behind, and what repairs it. Rendering lives in
//! `render`; these types carry only facts.
mod next;
use super::{
    ContentUnavailable, RewindArguments, RewindConflict,
    tree::{InvalidSourcePath, WriteConflict},
};
use enco_core::{IntentPath, PluginName, RegistrationId, code};
use enco_kernel::{LoadError, RegistryError, StoreError};
use serde::Serialize;
use std::path::PathBuf;

/// The development operation that failed; it decides what a failure left behind and how to
/// repeat the operation.
#[derive(Clone, Debug)]
pub enum Operation {
    Apply,
    Deploy,
    Startup,
    Rename { name: PluginName, to: PluginName },
    Rewind(RewindArguments),
    Scaffold,
    History,
    Status,
}

#[derive(Debug, thiserror::Error)]
#[error("{}", super::render::failure(self))]
pub struct Failed {
    pub operation: Operation,
    pub error: Box<DevelopmentError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{}: {problem}", path.display())]
pub struct EntryError {
    pub path: PathBuf,
    pub problem: EntryProblem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "problem", content = "detail", rename_all = "snake_case")]
pub enum EntryProblem {
    #[error("cannot be accessed: {0}")]
    Io(String),
    #[error("is a link or file, not a source directory")]
    NotDirectory,
    #[error("is not a regular file")]
    NotRegular,
    #[error("has a name that is not UTF-8")]
    NotUtf8,
    /// Source paths use `/` as their only separator, so names cannot contain `\\`.
    #[error("has a name that a source path cannot hold")]
    UnsupportedName,
    #[error("is not UTF-8 text")]
    NotText,
    #[error("has an invalid ignore rule: {0}")]
    IgnoreRule(String),
    #[error("is not a plugin name; use lowercase letters, digits and single hyphens")]
    NotPluginName,
    #[error("is not valid TOML: {0}")]
    Toml(String),
    #[error("has incomplete or out-of-order generated README markers")]
    ReadmeMarkers,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildFailure {
    #[error("cargo {stage} failed\n{}\nFull log: {}", .diagnostics.join("\n"), .log.display())]
    Cargo {
        stage: &'static str,
        /// Diagnostic excerpts, or the error output when Cargo reported none.
        diagnostics: Vec<String>,
        log: PathBuf,
    },
    #[error("Cargo produced no cdylib Wasm artifact\nFull log: {}", .0.display())]
    NoArtifact(PathBuf),
    #[error("empty configuration is invalid: {0}")]
    EmptySettings(LoadError),
    #[error("source changed during the build")]
    SourceChanged,
}

#[derive(Debug, thiserror::Error)]
pub enum DevelopmentError {
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("{name}: {failure}")]
    Build {
        name: PluginName,
        failure: BuildFailure,
    },
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("stored content is corrupt: {0}")]
    Corrupt(String),
    #[error("host failure: {0}")]
    Host(String),
    #[error("{name} is already registered or reserved")]
    NameTaken { name: PluginName },
    #[error("{} already exists", IntentPath::Plugin(name.clone()))]
    DirectoryExists { name: PluginName },
    #[error("{name} has no registered source directory")]
    NoSource { name: PluginName },
    /// Registered sources disappeared while new directories appeared; a rename is ambiguous.
    #[error("registered sources {missing:?} are missing while {added:?} appeared")]
    AmbiguousRename {
        missing: Vec<PluginName>,
        added: Vec<PluginName>,
        latest: RegistrationId,
    },
    /// Some sources or `config.toml` could not be prepared; each failure names its subject.
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Preparation(Vec<DevelopmentError>),
    /// The rename failed after its directory moved.
    #[error("after moving the directory: {0}")]
    Moved(Box<DevelopmentError>),
    /// The operation stopped after these file operations completed.
    #[error("stopped after {} file operations: {source}", .written.len())]
    Partial {
        written: Vec<PathBuf>,
        source: Box<DevelopmentError>,
    },
    #[error("rewind refused: {conflicts:?}")]
    RewindRefused {
        conflicts: Vec<RewindConflict>,
        /// Every path the rewind selected.
        selected: Vec<IntentPath>,
    },
    #[error("{path}: neither registration has a plugin under this name")]
    RewindPath { path: IntentPath },
    #[error("{path}: two plugins would be written here")]
    RewindCollision { path: IntentPath },
    #[error("{path}: {unavailable:?}")]
    RewindContent {
        path: IntentPath,
        unavailable: ContentUnavailable,
    },
    #[error("{}: {conflicts:?}", path.display())]
    WriteConflicts {
        path: PathBuf,
        conflicts: Vec<WriteConflict>,
    },
    #[error("cancelled")]
    Cancelled,
}

impl From<InvalidSourcePath> for DevelopmentError {
    fn from(error: InvalidSourcePath) -> Self {
        Self::Corrupt(error.to_string())
    }
}

impl Operation {
    pub(super) fn failed(self, error: impl Into<DevelopmentError>) -> Failed {
        Failed {
            operation: self,
            error: Box::new(error.into()),
        }
    }
}

impl Failed {
    pub fn code(&self) -> &'static str {
        self.error.code()
    }
}

impl DevelopmentError {
    /// The stable classification of the failure, decided where each variant is added.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => code::CANCELLED,
            Self::NameTaken { .. }
            | Self::DirectoryExists { .. }
            | Self::NoSource { .. }
            | Self::RewindPath { .. } => code::TOOL_INVALID_ARGUMENTS,
            Self::AmbiguousRename { .. } => code::PLUGIN_REJECTED,
            Self::Registry(error) => registry_code(error),
            Self::Moved(source) | Self::Partial { source, .. } => source.code(),
            Self::Entry(_)
            | Self::Build { .. }
            | Self::Store(_)
            | Self::Corrupt(_)
            | Self::Host(_)
            | Self::Preparation(_)
            | Self::RewindRefused { .. }
            | Self::RewindCollision { .. }
            | Self::RewindContent { .. }
            | Self::WriteConflicts { .. } => code::TOOL_FAILED,
        }
    }
}

fn registry_code(error: &RegistryError) -> &'static str {
    match error {
        RegistryError::UnknownPlugin(_) | RegistryError::UnknownRegistration { .. } => {
            code::TOOL_INVALID_ARGUMENTS
        }
        RegistryError::Rejected(_) | RegistryError::Conflict(_) => code::PLUGIN_REJECTED,
        RegistryError::NoRegistrations
        | RegistryError::Intent(_)
        | RegistryError::Store(_)
        | RegistryError::FactoryIdentity(_)
        | RegistryError::EmptyRequest
        | RegistryError::NotFactory(_)
        | RegistryError::FactoryRemoval(_)
        | RegistryError::UnknownGeneration(_)
        | RegistryError::UnknownProfile(_)
        | RegistryError::Config(_)
        | RegistryError::Bootstrap(_)
        | RegistryError::TableOwner(_)
        | RegistryError::NoRollbackTarget => code::TOOL_FAILED,
    }
}

pub(super) fn entry(path: impl Into<PathBuf>, problem: EntryProblem) -> EntryError {
    EntryError {
        path: path.into(),
        problem,
    }
}

pub(super) fn io(path: impl Into<PathBuf>, error: &std::io::Error) -> EntryError {
    entry(path, EntryProblem::Io(error.to_string()))
}
