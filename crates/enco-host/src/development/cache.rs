use crate::SqliteStore;
use crate::sqlite::{text, timestamp};
use enco_core::{ContentHash, DateTime, Utc};
use enco_kernel::StoreError;
use rusqlite::{OptionalExtension, params};

/// A successful local build; file retention is owned by the registry, independently of this record.
#[derive(Clone)]
pub(super) struct Build {
    pub source: ContentHash,
    pub sdk: ContentHash,
    pub artifact: ContentHash,
    pub toolchain: String,
    pub created_at: DateTime<Utc>,
}

fn error(error: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

impl SqliteStore {
    pub(super) async fn build(
        &self,
        source: ContentHash,
        sdk: ContentHash,
    ) -> Result<Option<Build>, StoreError> {
        self.run(move |connection| {
            connection
                .query_row(
                    "SELECT artifact,toolchain,created_at FROM builds WHERE source=? AND sdk=?",
                    params![source.to_string(), sdk.to_string()],
                    |row| {
                        Ok(Build {
                            source,
                            sdk,
                            artifact: text(row, 0)?,
                            toolchain: row.get(1)?,
                            created_at: text(row, 2)?,
                        })
                    },
                )
                .optional()
                .map_err(error)
        })
        .await
    }

    /// The stored navigation manual of a plugin source, as the manifest of its file tree.
    pub(super) async fn manual(
        &self,
        source: ContentHash,
    ) -> Result<Option<ContentHash>, StoreError> {
        self.run(move |connection| {
            connection
                .query_row(
                    "SELECT manual FROM manuals WHERE source=?",
                    params![source.to_string()],
                    |row| text(row, 0),
                )
                .optional()
                .map_err(error)
        })
        .await
    }

    pub(super) async fn remember_manual(
        &self,
        source: ContentHash,
        manual: ContentHash,
    ) -> Result<(), StoreError> {
        self.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO manuals(source,manual) VALUES (?,?)
                     ON CONFLICT(source) DO UPDATE SET manual=excluded.manual",
                    params![source.to_string(), manual.to_string()],
                )
                .map_err(error)?;
            Ok(())
        })
        .await
    }

    pub(super) async fn remember_build(&self, build: Build) -> Result<(), StoreError> {
        self.run(move |connection| {
            connection.execute(
                "INSERT INTO builds(source,sdk,artifact,toolchain,created_at) VALUES (?,?,?,?,?)
                 ON CONFLICT(source,sdk) DO UPDATE SET
                   artifact=excluded.artifact, toolchain=excluded.toolchain, created_at=excluded.created_at",
                params![build.source.to_string(), build.sdk.to_string(), build.artifact.to_string(),
                    build.toolchain, timestamp(build.created_at)],
            ).map_err(error)?;
            Ok(())
        }).await
    }
}
