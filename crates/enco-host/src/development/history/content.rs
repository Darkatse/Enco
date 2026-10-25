//! The files of each unit at one side of a comparison: what a registration stored, or what the
//! home holds now.
use super::{Development, DevelopmentError, HistoryPoint};
use crate::development::{EntryError, error::io, tree::FileTree};
use enco_core::*;
use enco_kernel::{RegisteredIntent, Registry, Store, StoreError};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(super) enum Unit {
    Configuration,
    Plugin(PluginId),
    Unregistered(PluginName),
}
pub(super) struct Content {
    pub(super) path: IntentPath,
    pub(super) value: Result<FileTree, ContentUnavailable>,
}
pub(super) type Contents = BTreeMap<Unit, Content>;

/// One side's content that cannot be read, and why.
#[derive(Debug, Serialize)]
pub struct ContentUnavailable {
    /// The side whose content cannot be read.
    pub point: HistoryPoint,
    /// Why it cannot be read.
    pub cause: ContentFailure,
}

/// Why content cannot be read; other failures stop the comparison.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentFailure {
    /// Enco's store no longer has the content; stored blobs are removable history.
    Missing {
        /// The address of the missing content.
        hash: ContentHash,
    },
    /// The stored content cannot be read back.
    Corrupt {
        /// What is malformed.
        reason: String,
    },
    /// The files in the home cannot be read.
    Unreadable(EntryError),
}

impl ContentUnavailable {
    /// Classify a read failure of one side; failures of the store itself are not content facts.
    pub(in crate::development) fn of(
        point: HistoryPoint,
        error: DevelopmentError,
    ) -> Result<Self, DevelopmentError> {
        let cause = match error {
            DevelopmentError::Store(StoreError::Blob(hash)) => ContentFailure::Missing { hash },
            DevelopmentError::Corrupt(reason) => ContentFailure::Corrupt { reason },
            DevelopmentError::Entry(error) => ContentFailure::Unreadable(error),
            error @ (DevelopmentError::Store(_)
            | DevelopmentError::Build { .. }
            | DevelopmentError::Registry(_)
            | DevelopmentError::Host(_)
            | DevelopmentError::NameTaken { .. }
            | DevelopmentError::DirectoryExists { .. }
            | DevelopmentError::NoSource { .. }
            | DevelopmentError::AmbiguousRename { .. }
            | DevelopmentError::Preparation(_)
            | DevelopmentError::Moved(_)
            | DevelopmentError::Partial { .. }
            | DevelopmentError::RewindRefused { .. }
            | DevelopmentError::RewindPath { .. }
            | DevelopmentError::RewindCollision { .. }
            | DevelopmentError::RewindContent { .. }
            | DevelopmentError::WriteConflicts { .. }
            | DevelopmentError::Cancelled) => return Err(error),
        };
        Ok(Self { point, cause })
    }
}

impl Development {
    pub(super) async fn contents(
        &self,
        registry: &Registry,
        point: HistoryPoint,
        names: &RegisteredIntent,
    ) -> Result<Contents, DevelopmentError> {
        let read = |value: Result<FileTree, DevelopmentError>| match value {
            Ok(tree) => Ok(Ok(tree)),
            Err(error) => ContentUnavailable::of(point, error).map(Err),
        };
        let mut contents = BTreeMap::new();
        match point {
            HistoryPoint::Registration(id) => {
                let intent = registry.registered_intent(Some(id)).await?;
                let config = &intent.accepted()?.configuration;
                let value = self
                    .store
                    .get_blob(&config.source)
                    .await
                    .map_err(DevelopmentError::from)
                    .and_then(|bytes| FileTree::from_files([(CONFIGURATION_FILE, bytes)]));
                contents.insert(
                    Unit::Configuration,
                    Content {
                        path: IntentPath::Configuration,
                        value: read(value)?,
                    },
                );
                for (name, plugin) in &intent.plugins {
                    if let Some(source) = plugin.source() {
                        let name = names.plugin_by_id(plugin.id).map_or(name, |(name, _)| name);
                        let value = FileTree::load(self.store.as_ref(), source).await;
                        contents.insert(
                            Unit::Plugin(plugin.id),
                            Content {
                                path: IntentPath::Plugin(name.clone()),
                                value: read(value)?,
                            },
                        );
                    }
                }
            }
            HistoryPoint::Home => {
                let location = IntentPath::Configuration.locate(&self.home);
                let value = tokio::fs::read(&location)
                    .await
                    .map_err(|error| DevelopmentError::from(io(&location, &error)))
                    .and_then(|bytes| FileTree::from_files([(CONFIGURATION_FILE, bytes)]));
                contents.insert(
                    Unit::Configuration,
                    Content {
                        path: IntentPath::Configuration,
                        value: read(value)?,
                    },
                );
                for name in self.directories().await? {
                    let unit = names.plugins.get(&name).map_or_else(
                        || Unit::Unregistered(name.clone()),
                        |plugin| Unit::Plugin(plugin.id),
                    );
                    let path = IntentPath::Plugin(name);
                    let value = FileTree::read(&path.locate(&self.home)).await;
                    contents.insert(
                        unit,
                        Content {
                            path,
                            value: read(value)?,
                        },
                    );
                }
            }
        }
        Ok(contents)
    }
}
