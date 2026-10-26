//! Turns the plugin sources and `config.toml` in a home into registrations, and registered versions
//! back into files.
//!
//! The owner and the Agent edit those files; what is registered and what runs live in Enco's store,
//! and only the registry changes them.
mod arguments;
mod build;
pub use arguments::{
    AgentReason, ApplyArguments, RenameArguments, RewindArguments, ScaffoldArguments,
};
mod call;
pub use call::{
    APPLY, DevelopmentCall, DevelopmentTool, HISTORY, Invocation, RENAME, REWIND, SCAFFOLD, STATUS,
    StatusArguments, SuggestedArguments, self_status_spec,
};
mod cache;
mod error;
pub use error::{
    BuildFailure, DevelopmentError, EntryError, EntryProblem, Failed, Operation, ToolchainProblem,
};
mod history;
mod home;
mod intent;
mod manual;
mod navigation;
pub mod tree;
pub use history::{
    Comparison, ComparisonMode, ContentFailure, ContentUnavailable, FileDiff, FileDifference,
    HISTORY_PAGE_SIZE, History, HistoryPoint, HistoryQuery, UnitDiff,
};
mod process;
mod readme;
mod registration;
mod rename;
mod render;
pub use render::registration_document;
mod rewind;
pub use rewind::{RewindConflict, Rewound};
mod scaffold;
mod sdk;
mod status;

use crate::SqliteStore;
pub use build::BuildResult;
use enco_core::{IntentPath, PLUGINS_DIRECTORY, PluginId};
use enco_kernel::{Cause, Registered, Runtime};
pub use home::{HomeComparison, HomePlugin, SettingsComparison};
pub use manual::{DIRECTORY as MANUAL_DIRECTORY, Following, Manual};
pub use scaffold::{FactoryFiles, FactorySources};
pub use sdk::Sdk;
use serde::Serialize;
pub use status::{ConfigurationUnit, DevelopmentStatus, PluginUnit, RegisteredUnit};
use std::collections::BTreeMap;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// Entry point for the plugin development tools in one home.
///
/// Operations that write files or register run one at a time; status and history do not wait for
/// them.
pub struct Development {
    home: PathBuf,
    sdk: Sdk,
    factory: FactorySources,
    manual: Arc<Manual>,
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    operation: Mutex<()>,
    follow_rejections: RwLock<BTreeMap<PluginId, Vec<Cause>>>,
}

/// The result of an apply: what was registered, and what each build did.
#[derive(Serialize)]
pub struct Applied {
    /// The registration; absent when nothing differs from what is registered and no factory plugin
    /// has newer host code to follow.
    pub registered: Option<Registered>,
    /// One result for each plugin source that differs from what is registered or was built with an
    /// earlier SDK; unchanged sources are skipped.
    pub builds: Vec<BuildResult>,
    /// Plugin directories or `config.toml` whose bytes changed but whose meaning did not, as when
    /// only a comment was edited; they stay unregistered.
    pub text_only: Vec<IntentPath>,
}

impl Development {
    /// Bind development to `home`; nothing is read or written until an operation runs.
    pub fn new(
        home: PathBuf,
        sdk: Sdk,
        factory: FactorySources,
        manual: Manual,
        store: Arc<SqliteStore>,
        runtime: Arc<dyn Runtime>,
    ) -> Arc<Self> {
        Arc::new(Self {
            home,
            sdk,
            factory,
            manual: Arc::new(manual),
            store,
            runtime,
            operation: Mutex::new(()),
            follow_rejections: RwLock::new(BTreeMap::new()),
        })
    }

    /// The manual of this home, which every registration made here rewrites before it returns.
    pub fn manual(&self) -> &Arc<Manual> {
        &self.manual
    }

    /// Create `plugins/` in a new home; an existing directory is left as it is.
    pub async fn initialize(home: &Path) -> Result<(), EntryError> {
        let plugins = home.join(PLUGINS_DIRECTORY);
        tokio::fs::create_dir_all(&plugins)
            .await
            .map_err(|error| error::io(plugins, &error))
    }
}

fn check_cancelled(cancel: &CancellationToken) -> Result<(), DevelopmentError> {
    if cancel.is_cancelled() {
        Err(DevelopmentError::Cancelled)
    } else {
        Ok(())
    }
}
