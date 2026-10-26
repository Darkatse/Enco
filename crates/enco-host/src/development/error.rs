//! What failed, what the failed operation left behind, and what repairs it.
//!
//! These types carry only facts; `render` turns them into text.
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
    /// Registering the home as it is now, after building the plugins whose source changed.
    Apply,
    /// Restoring registered behavior as Enco starts, and registering a new home the first time.
    Startup,
    /// Renaming a plugin together with its directory.
    Rename {
        /// The plugin's current name.
        name: PluginName,
        /// The name it was to take.
        to: PluginName,
    },
    /// Writing parts of the home back to what an earlier registration registered.
    Rewind(RewindArguments),
    /// Creating a plugin directory from the template.
    Scaffold,
    /// Reading the registration history or comparing two points in it.
    History,
    /// Reading how the home, the registrations and what runs compare.
    Status,
}

/// A failed development operation; its message says what failed, what was left behind, and what
/// to do next.
#[derive(Debug, thiserror::Error)]
#[error("{}", super::render::failure(self))]
pub struct Failed {
    /// The operation that failed, which decides how to repeat it.
    pub operation: Operation,
    /// Why it failed.
    pub error: Box<DevelopmentError>,
}

/// A file or directory in the home that cannot be used as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{}: {problem}", path.display())]
pub struct EntryError {
    /// The file or directory.
    pub path: PathBuf,
    /// What is wrong with it.
    pub problem: EntryProblem,
}

/// What is wrong with a file or directory in the home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "problem", content = "detail", rename_all = "snake_case")]
pub enum EntryProblem {
    /// The operating system refused to read or write it; the text is its message.
    #[error("cannot be accessed: {0}")]
    Io(String),
    /// A plugin's source is a file or a symbolic link where a directory must be.
    #[error("is a link or file, not a source directory")]
    NotDirectory,
    /// A source directory holds something other than files and directories, such as a symbolic
    /// link.
    #[error("is not a regular file")]
    NotRegular,
    /// Its name is not valid UTF-8.
    #[error("has a name that is not UTF-8")]
    NotUtf8,
    /// Its name cannot be part of a source path.
    ///
    /// Source paths use `/` as their only separator, so names cannot contain `\\`.
    #[error("has a name that a source path cannot hold")]
    UnsupportedName,
    /// Its content is not UTF-8 text.
    #[error("is not UTF-8 text")]
    NotText,
    /// A `.gitignore` rule cannot be parsed; the text is the parser's message.
    #[error("has an invalid ignore rule: {0}")]
    IgnoreRule(String),
    /// A directory under `plugins/` whose name is not a valid plugin name.
    #[error("is not a plugin name; use {rule}", rule = PluginName::RULE)]
    NotPluginName,
    /// `config.toml` cannot be parsed; the text is the parser's message.
    #[error("is not valid TOML: {0}")]
    Toml(String),
    /// A plugin's README has only one marker of its generated section, or both in the wrong order.
    #[error("has incomplete or out-of-order generated README markers")]
    ReadmeMarkers,
}

/// Why one plugin's source did not build.
#[derive(Debug, thiserror::Error)]
pub enum BuildFailure {
    /// A Cargo command failed.
    #[error("cargo {stage} failed\n{}\nFull log: {}\nLogs are retained until the build directory is cleaned", .diagnostics.join("\n"), .log.display())]
    Cargo {
        /// The command: `clippy`, `build` or `doc`.
        stage: &'static str,
        /// Diagnostic excerpts, or the error output when Cargo reported none.
        diagnostics: Vec<String>,
        /// Where Cargo's complete output is stored.
        log: PathBuf,
    },
    /// Cargo succeeded but produced no Wasm component for the plugin; the path is the stored log.
    #[error("Cargo produced no cdylib Wasm artifact\nFull log: {}\nLogs are retained until the build directory is cleaned", .0.display())]
    NoArtifact(PathBuf),
    /// The component does not load with an empty configuration, which every plugin must accept.
    #[error("empty configuration is invalid: {0}")]
    EmptySettings(LoadError),
    /// Files in the source directory changed while the build was reading them.
    #[error("source changed during the build")]
    SourceChanged,
    /// The pinned Rust toolchain cannot build plugins on this machine.
    ///
    /// This is checked before Cargo runs. Installing a toolchain downloads it, so the owner repairs
    /// this.
    #[error("Rust {channel}: {problem}")]
    Toolchain {
        /// The toolchain the SDK pins, as rustup names it.
        channel: String,
        /// What is missing or wrong.
        problem: ToolchainProblem,
    },
}

/// Why the SDK's toolchain cannot build plugins on this machine.
#[derive(Debug, thiserror::Error)]
pub enum ToolchainProblem {
    /// `rustc` fails under the pinned toolchain.
    #[error("not installed")]
    Missing,
    /// The toolchain has no standard library for `wasm32-wasip2`.
    #[error("the plugin target is not installed")]
    Target,
    /// The `rustc` Cargo would use does not go through rustup, so it ignores the pinned toolchain.
    #[error("rustc is release {found}")]
    Release {
        /// The release that `rustc -vV` reports.
        found: String,
    },
}

/// Why a development operation failed.
#[derive(Debug, thiserror::Error)]
pub enum DevelopmentError {
    /// A file or directory in the home cannot be used as it is.
    #[error(transparent)]
    Entry(#[from] EntryError),
    /// A plugin's source did not build.
    #[error("{name}: {failure}")]
    Build {
        /// The plugin whose source did not build.
        name: PluginName,
        /// How the build failed.
        failure: BuildFailure,
    },
    /// The registry rejected the change or could not complete it.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Enco's store could not read or write.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Content in Enco's store cannot be read back, such as a malformed file manifest.
    #[error("stored content is corrupt: {0}")]
    Corrupt(String),
    /// The host's own machinery failed, such as starting a process or reading its embedded files.
    #[error("host failure: {0}")]
    Host(String),
    /// A new plugin was asked for under a name that is already registered or reserved.
    #[error("{name} is already registered or reserved")]
    NameTaken {
        /// The name asked for.
        name: PluginName,
    },
    /// The directory that a new or renamed plugin would take already exists.
    #[error("{} already exists", IntentPath::Plugin(name.clone()))]
    DirectoryExists {
        /// The name whose directory exists.
        name: PluginName,
    },
    /// A rename was asked for a plugin with no registered source directory to move, such as one
    /// that follows factory code.
    #[error("{name} has no registered source directory")]
    NoSource {
        /// The plugin asked to be renamed.
        name: PluginName,
    },
    /// Registered sources disappeared while new directories appeared; a rename is ambiguous.
    #[error("registered sources {missing:?} are missing while {added:?} appeared")]
    AmbiguousRename {
        /// Registered plugins whose directories are gone.
        missing: Vec<PluginName>,
        /// Directories under `plugins/` that no registration has.
        added: Vec<PluginName>,
        /// The latest registration, from which the missing directories can be restored.
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
        /// The files and directories written before it stopped.
        written: Vec<PathBuf>,
        /// Why it stopped.
        source: Box<DevelopmentError>,
    },
    /// A rewind would overwrite or remove something no registration holds; nothing was written.
    #[error("rewind refused: {conflicts:?}")]
    RewindRefused {
        /// What stands in the way, for each path.
        conflicts: Vec<RewindConflict>,
        /// Every path the rewind selected.
        selected: Vec<IntentPath>,
    },
    /// A rewind named a plugin that neither the latest nor the target registration has.
    #[error("{path}: neither registration has a plugin under this name")]
    RewindPath {
        /// The path as the rewind named it.
        path: IntentPath,
    },
    /// A rewind would write two plugins to the same directory.
    #[error("{path}: two plugins would be written here")]
    RewindCollision {
        /// The directory both would be written to.
        path: IntentPath,
    },
    /// A rewind needs registered content that can no longer be read from Enco's store.
    #[error("{path}: {unavailable:?}")]
    RewindContent {
        /// The path the content was to be written to.
        path: IntentPath,
        /// Why the content cannot be read.
        unavailable: ContentUnavailable,
    },
    /// Writing a directory would destroy something the write does not own, such as an ignored file.
    #[error("{}: {conflicts:?}", path.display())]
    WriteConflicts {
        /// The directory being written.
        path: PathBuf,
        /// What stands in the way.
        conflicts: Vec<WriteConflict>,
    },
    /// The caller cancelled the operation before it finished.
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
    /// The stable classification of the error that failed the operation.
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
