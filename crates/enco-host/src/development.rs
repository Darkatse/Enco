//! Source intent belongs to the home repository; only the registry changes running behavior.
mod build;
mod cache;
mod git;
mod intent;
mod process;
mod readme;
mod registration;
mod scaffold;
mod sdk;
mod status;

use crate::SqliteStore;
pub use build::BuildResult;
use enco_core::PluginId;
use enco_kernel::{Registered, RegistryError, Runtime, StoreError};
pub use sdk::Sdk;
use serde::Serialize;
pub use status::{PluginWorkspace, RegisteredComparison, WorkspaceStatus};
use std::collections::BTreeMap;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// Serializes builds and Git operations over one owner's intent directory.
pub struct Development {
    home: PathBuf,
    sdk: Sdk,
    git: git::Git,
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    operation: Mutex<()>,
    follow_rejections: RwLock<BTreeMap<PluginId, String>>,
}

/// Build evidence and the accepted change returned by both management entry points.
#[derive(Serialize)]
pub struct Applied {
    /// Accepted change, absent when the workspace intent is unchanged.
    pub registered: Option<Registered>,
    /// Build and reuse evidence for each source directory considered.
    pub builds: Vec<BuildResult>,
}

/// Failures retain the boundary responsible for the operation.
#[derive(Debug, thiserror::Error)]
pub enum DevelopmentError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{}: {reason}", path.display())]
    Input { path: PathBuf, reason: String },
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Build(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Format(#[from] std::fmt::Error),
    #[error("source moved from {from} to {to}, but the rename was not registered: {source}")]
    Rename {
        from: String,
        to: String,
        source: Box<DevelopmentError>,
    },
    #[error("operation cancelled")]
    Cancelled,
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
            git: git::Git::new(&home),
            home,
            sdk,
            store,
            runtime,
            operation: Mutex::new(()),
            follow_rejections: RwLock::new(BTreeMap::new()),
        })
    }

    /// Create a repository without replacing existing intent files.
    pub async fn initialize(home: &Path) -> Result<(), DevelopmentError> {
        tokio::fs::create_dir_all(home.join("plugins")).await?;
        let ignore = home.join(".gitignore");
        let mut text = match tokio::fs::read_to_string(&ignore).await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        for entry in ["/.data/", "/workspace/", "/sdk/", "target/"] {
            if !text.lines().any(|line| line == entry) {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(entry);
                text.push('\n');
            }
        }
        tokio::fs::write(ignore, text).await?;
        match tokio::fs::remove_file(home.join("plugins.lock")).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        git::Git::new(home).initialize().await
    }
}

fn check_cancelled(cancel: &CancellationToken) -> Result<(), DevelopmentError> {
    if cancel.is_cancelled() {
        Err(DevelopmentError::Cancelled)
    } else {
        Ok(())
    }
}
