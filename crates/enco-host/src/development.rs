//! The home holds editable intent; accepted bytes and running behavior belong to Enco.
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
pub use error::{BuildFailure, DevelopmentError, EntryError, EntryProblem, Failed, Operation};
mod history;
mod intent;
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
mod workspace;

use crate::SqliteStore;
pub use build::BuildResult;
use enco_core::{IntentPath, PLUGINS_DIRECTORY, PluginId};
use enco_kernel::{Cause, Registered, Runtime};
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
pub use workspace::{PluginWorkspace, SettingsComparison, WorkspaceComparison};

/// Serializes builds and intent operations over one owner's intent directory.
pub struct Development {
    home: PathBuf,
    sdk: Sdk,
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    operation: Mutex<()>,
    follow_rejections: RwLock<BTreeMap<PluginId, Vec<Cause>>>,
}

/// Build evidence and the accepted change returned by both management entry points.
#[derive(Serialize)]
pub struct Applied {
    /// Accepted change, absent when the workspace intent is unchanged.
    pub registered: Option<Registered>,
    /// Build and reuse evidence for each source directory considered.
    pub builds: Vec<BuildResult>,
    /// Byte changes that remain in the workspace because registered meaning is unchanged.
    pub text_only: Vec<IntentPath>,
}

impl Development {
    /// Assemble the owner of local source operations; startup writes the SDK through its bundle.
    pub fn new(
        home: PathBuf,
        sdk: Sdk,
        store: Arc<SqliteStore>,
        runtime: Arc<dyn Runtime>,
    ) -> Arc<Self> {
        Arc::new(Self {
            home,
            sdk,
            store,
            runtime,
            operation: Mutex::new(()),
            follow_rejections: RwLock::new(BTreeMap::new()),
        })
    }

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
