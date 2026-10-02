use super::MemoryError;
use crate::sqlite;
use enco_core::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// `PRAGMA user_version` of the memories table; follows the same rule as `enco.db`.
const SCHEMA_VERSION: u32 = 1;
const COLUMNS: &str = "id,text,pinned,created_at,updated_at,rev";
pub(super) struct Authority {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Clone)]
pub(super) struct Revision {
    pub id: MemoryId,
    pub rev: u64,
}

fn error(e: impl std::fmt::Display) -> MemoryError {
    MemoryError::Authority(e.to_string())
}

fn memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: sqlite::text(row, 0)?,
        text: row.get(1)?,
        pinned: row.get(2)?,
        created_at: sqlite::text(row, 3)?,
        updated_at: sqlite::text(row, 4)?,
        rev: sqlite::unsigned(row, 5)?,
    })
}

impl Authority {
    pub async fn open(path: PathBuf) -> Result<Self, MemoryError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(error)?;
        }
        let connection = tokio::task::spawn_blocking(move || {
            let mut connection = sqlite::open(&path).map_err(error)?;
            match sqlite::schema_version(&connection).map_err(error)? {
                None => {
                    let tx = connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(error)?;
                    tx.execute_batch(
                        "CREATE TABLE memories(
                         id TEXT PRIMARY KEY, text TEXT NOT NULL,
                         pinned INTEGER NOT NULL CHECK(pinned IN(0,1)),
                         created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                         rev INTEGER NOT NULL
                         ) STRICT;",
                    )
                    .map_err(error)?;
                    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                        .map_err(error)?;
                    tx.commit().map_err(error)?;
                }
                Some(SCHEMA_VERSION) => {}
                Some(found) => {
                    return Err(MemoryError::SchemaVersion {
                        found,
                        expected: SCHEMA_VERSION,
                    });
                }
            }
            Ok(connection)
        })
        .await
        .map_err(error)??;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    async fn run<T: Send + 'static>(
        &self,
        call: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T, MemoryError> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let connection = connection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            call(&connection).map_err(error)
        })
        .await
        .map_err(error)?
    }

    pub async fn save(
        &self,
        text: String,
        pinned: bool,
        now: DateTime<Utc>,
    ) -> Result<Memory, MemoryError> {
        self.run(move |connection| {
            connection.query_row(
                &format!("INSERT INTO memories VALUES(?,?,?,?,?,1) RETURNING {COLUMNS}"),
                params![
                    MemoryId::new().to_string(),
                    text,
                    pinned,
                    sqlite::timestamp(now),
                    sqlite::timestamp(now)
                ],
                memory,
            )
        })
        .await
    }

    pub async fn update(
        &self,
        id: MemoryId,
        text: Option<String>,
        pinned: Option<bool>,
        now: DateTime<Utc>,
    ) -> Result<Option<Memory>, MemoryError> {
        self.run(move |connection| {
            connection
                .query_row(
                    &format!(
                        "UPDATE memories SET text=COALESCE(?,text),pinned=COALESCE(?,pinned),
                         updated_at=?,rev=rev+1
                         WHERE id=? RETURNING {COLUMNS}"
                    ),
                    params![text, pinned, sqlite::timestamp(now), id.to_string()],
                    memory,
                )
                .optional()
        })
        .await
    }

    pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError> {
        self.run(move |connection| {
            connection
                .execute("DELETE FROM memories WHERE id=?", [id.to_string()])
                .map(|n| n > 0)
        })
        .await
    }

    pub async fn all(&self, pinned_only: bool) -> Result<Vec<Memory>, MemoryError> {
        self.run(move |connection| {
            let mut query = connection.prepare(&format!(
                "SELECT {COLUMNS}
                 FROM memories
                 WHERE (?=0 OR pinned=1)
                 ORDER BY pinned DESC,updated_at DESC,id"
            ))?;
            query.query_map([pinned_only], memory)?.collect()
        })
        .await
    }

    /// Newest updates first, with the ID breaking timestamp ties deterministically.
    pub async fn revisions(&self) -> Result<Vec<Revision>, MemoryError> {
        self.run(|connection| {
            let mut query =
                connection.prepare("SELECT id,rev FROM memories ORDER BY updated_at DESC,id")?;
            query
                .query_map([], |row| {
                    Ok(Revision {
                        id: sqlite::text(row, 0)?,
                        rev: sqlite::unsigned(row, 1)?,
                    })
                })?
                .collect()
        })
        .await
    }

    pub async fn get(&self, ids: Vec<MemoryId>) -> Result<Vec<Memory>, MemoryError> {
        self.run(move |connection| {
            let mut query =
                connection.prepare(&format!("SELECT {COLUMNS} FROM memories WHERE id=?"))?;
            let mut result = vec![];
            for id in ids {
                if let Some(row) = query.query_row([id.to_string()], memory).optional()? {
                    result.push(row);
                }
            }
            Ok(result)
        })
        .await
    }
}
