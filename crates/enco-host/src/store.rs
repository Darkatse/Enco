mod accept;
mod commit;
mod content;
mod lock;
mod registry;
mod rows;

use crate::{limits::RECENT_DELIVERY_FAILURES, sqlite::timestamp};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const SESSION_COLUMNS: &str = "id,name,created_at,binding_node,binding_epoch,profile";
const SCHEDULE_COLUMNS: &str = "id,session_id,due_at,message,created_at,state,fired_event_id";

/// Files owned by the persistence adapter.
pub struct StorePaths {
    /// SQLite authority.
    pub db: PathBuf,
    /// Removable content-addressed history.
    pub blobs: PathBuf,
    /// Retained content-addressed components.
    pub artifacts: PathBuf,
    /// Authoritative plugin name-to-identity mapping.
    pub plugins_lock: PathBuf,
}

/// SQLite authority and its associated content and identity files.
pub struct SqliteStore {
    connection: Arc<Mutex<Connection>>,
    paths: StorePaths,
    node: NodeId,
}

impl SqliteStore {
    /// Open the authority, initializing the schema and node identity on first use.
    pub async fn open(paths: StorePaths) -> Result<Self, StoreError> {
        for path in [&paths.db, &paths.plugins_lock] {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(backend)?;
            }
        }
        tokio::fs::create_dir_all(&paths.blobs)
            .await
            .map_err(backend)?;
        tokio::fs::create_dir_all(&paths.artifacts)
            .await
            .map_err(backend)?;
        let db = paths.db.clone();
        let (connection, node) = tokio::task::spawn_blocking(move || open(&db))
            .await
            .map_err(backend)??;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            paths,
            node,
        })
    }

    fn artifact_path(&self, hash: &ContentHash) -> PathBuf {
        self.paths.artifacts.join(format!("{hash}.wasm"))
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            operation(&mut connection)
        })
        .await
        .map_err(backend)?
    }
}

fn backend(e: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(e.to_string())
}

fn encode(value: &impl serde::Serialize) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(backend)
}

fn open(path: &Path) -> Result<(Connection, NodeId), StoreError> {
    let mut connection = crate::sqlite::open(path).map_err(backend)?;
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
            [],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if !exists {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        tx.execute_batch(include_str!("store/schema.sql"))
            .map_err(backend)?;
        tx.execute(
            "INSERT INTO meta(key,value) VALUES ('schema_version','1'),('node_id',?),('safe_mode','0')",
            [NodeId::new().to_string()],
        ).map_err(backend)?;
        tx.commit().map_err(backend)?;
    }
    let version: String = connection
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(backend)?;
    let version: u32 = version.parse().map_err(backend)?;
    if version > 1 {
        return Err(StoreError::NewerSchema(version));
    }
    if version != 1 {
        return Err(backend(format!("unsupported schema version {version}")));
    }
    let node = connection
        .query_row("SELECT value FROM meta WHERE key='node_id'", [], |r| {
            rows::text(r, 0)
        })
        .map_err(backend)?;
    Ok((connection, node))
}

#[async_trait]
impl Store for SqliteStore {
    async fn node(&self) -> Result<NodeRecord, StoreError> {
        let id = self.node;
        self.run(move |connection| {
            let value: String = connection
                .query_row("SELECT value FROM meta WHERE key='safe_mode'", [], |r| {
                    r.get(0)
                })
                .map_err(backend)?;
            Ok(NodeRecord {
                id,
                safe_mode: match value.as_str() {
                    "0" => false,
                    "1" => true,
                    _ => return Err(backend("invalid safe_mode value")),
                },
            })
        })
        .await
    }

    async fn set_safe_mode(&self, enabled: bool) -> Result<(), StoreError> {
        self.run(move |connection| {
            connection
                .execute(
                    "UPDATE meta SET value=? WHERE key='safe_mode'",
                    [if enabled { "1" } else { "0" }],
                )
                .map_err(backend)?;
            Ok(())
        })
        .await
    }

    async fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        self.run(|connection| {
            let mut query = connection
                .prepare(&format!(
                    "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY name"
                ))
                .map_err(backend)?;
            query
                .query_map([], rows::session)
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)
        })
        .await
    }

    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError> {
        let name = name.to_owned();
        self.run(move |connection| {
            connection
                .query_row(
                    &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE name=?"),
                    [name],
                    rows::session,
                )
                .optional()
                .map_err(backend)
        })
        .await
    }

    async fn session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError> {
        self.run(move |connection| {
            connection
                .query_row(
                    &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id=?"),
                    [id.to_string()],
                    rows::session,
                )
                .optional()
                .map_err(backend)
        })
        .await
    }

    async fn set_profile(&self, id: SessionId, profile: &str) -> Result<(), StoreError> {
        let profile = profile.to_owned();
        self.run(move |connection| {
            let changed = connection
                .execute(
                    "UPDATE sessions SET profile=? WHERE id=?",
                    params![profile, id.to_string()],
                )
                .map_err(backend)?;
            if changed == 0 {
                return Err(StoreError::UnknownSession(id));
            }
            Ok(())
        })
        .await
    }

    async fn ensure_session(
        &self,
        name: &str,
        created_at: DateTime<Utc>,
    ) -> Result<SessionRecord, StoreError> {
        let name = name.to_owned();
        let node = self.node;
        self.run(move |connection| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(backend)?;
            tx.execute(
                "INSERT OR IGNORE INTO sessions VALUES (?,?,?,?,?,?)",
                params![
                    SessionId::new().to_string(),
                    name,
                    timestamp(created_at),
                    node.to_string(),
                    1,
                    DEFAULT_PROFILE
                ],
            )
            .map_err(backend)?;
            let result = tx
                .query_row(
                    &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE name=?"),
                    [name],
                    rows::session,
                )
                .map_err(backend)?;
            tx.commit().map_err(backend)?;
            Ok(result)
        })
        .await
    }

    async fn log(
        &self,
        session: SessionId,
        after: Option<LogPos>,
    ) -> Result<Vec<Entry>, StoreError> {
        self.run(move |connection| {
            let position = after.unwrap_or(LogPos {
                epoch: Epoch(0),
                seq: Seq(0),
            });
            let mut query = connection
                .prepare(
                    "SELECT epoch,seq,at,body
                     FROM log
                     WHERE session_id=? AND (epoch,seq)>(?,?)
                     ORDER BY epoch,seq",
                )
                .map_err(backend)?;
            query
                .query_map(
                    params![
                        session.to_string(),
                        signed(position.epoch.0)?,
                        signed(position.seq.0)?
                    ],
                    rows::entry,
                )
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)
        })
        .await
    }

    async fn commit(&self, session: SessionId, commit: Commit) -> Result<(), StoreError> {
        let node = self.node;
        self.run(move |connection| commit::commit(connection, node, session, commit))
            .await
    }

    async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Vec<Accepted>, StoreError> {
        let events = events.to_vec();
        let write = connection.cloned();
        self.run(move |connection| accept::accept(connection, &events, write.as_ref()))
            .await
    }

    async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, StoreError> {
        let key = key.to_owned();
        self.run(move |connection| {
            connection
                .query_row("SELECT state FROM connections WHERE key=?", [key], |r| {
                    rows::document(r, 0)
                })
                .optional()
                .map_err(backend)
        })
        .await
    }

    async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, StoreError> {
        let key = key.to_owned();
        self.run(move |connection| {
            let mut query = connection.prepare(
                "SELECT body FROM deliveries WHERE connection=? AND outcome IN ('failed', 'unknown') ORDER BY order_no DESC LIMIT ?"
            ).map_err(backend)?;
            query.query_map(params![key, RECENT_DELIVERY_FAILURES], |r| rows::document(r, 0))
                .map_err(backend)?.collect::<Result<Vec<_>, _>>().map_err(backend)
        }).await
    }

    async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError> {
        self.run(move |connection| {
            let mut query = connection
                .prepare(
                    "SELECT event
                     FROM inbox
                     WHERE session_id=? AND consumed_seq IS NULL
                     ORDER BY order_no",
                )
                .map_err(backend)?;
            query
                .query_map([session.to_string()], |r| rows::document(r, 0))
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)
        })
        .await
    }

    async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        let schedule = schedule.clone();
        self.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO schedules VALUES (?,?,?,?,?,'pending',NULL)",
                    params![
                        schedule.id.to_string(),
                        schedule.session.to_string(),
                        timestamp(schedule.due_at),
                        schedule.message,
                        timestamp(schedule.created_at)
                    ],
                )
                .map_err(backend)?;
            Ok(())
        })
        .await
    }

    async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError> {
        self.run(move |connection| {
            connection
                .execute(
                    "UPDATE schedules SET state='cancelled' WHERE id=? AND state='pending'",
                    [id.to_string()],
                )
                .map(|n| n > 0)
                .map_err(backend)
        })
        .await
    }

    async fn schedules(
        &self,
        state: Option<ScheduleStateKind>,
    ) -> Result<Vec<Schedule>, StoreError> {
        self.run(move |connection| {
            let filter = state.map(|schedule| match schedule {
                ScheduleStateKind::Pending => "pending",
                ScheduleStateKind::Fired => "fired",
                ScheduleStateKind::Cancelled => "cancelled",
            });
            let mut query = connection
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLUMNS}
                     FROM schedules
                     WHERE (? IS NULL OR state=?)
                     ORDER BY due_at,id"
                ))
                .map_err(backend)?;
            query
                .query_map(params![filter, filter], rows::schedule)
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)
        })
        .await
    }

    async fn fire_schedule(&self, id: ScheduleId, event: &Event) -> Result<(), StoreError> {
        let event = event.clone();
        self.run(move |connection| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(backend)?;
            let inserted = tx
                .execute(
                    "UPDATE schedules SET state='fired',fired_event_id=?
                     WHERE id=? AND session_id=? AND state='pending'",
                    params![
                        event.id.to_string(),
                        id.to_string(),
                        event.session.to_string()
                    ],
                )
                .map_err(backend)?;
            if inserted == 0 {
                return Err(StoreError::ScheduleNotPending(id));
            }
            tx.execute(
                "INSERT INTO inbox(event_id,session_id,event) VALUES (?,?,?)",
                params![
                    event.id.to_string(),
                    event.session.to_string(),
                    encode(&event)?
                ],
            )
            .map_err(backend)?;
            tx.commit().map_err(backend)
        })
        .await
    }

    async fn plugin_names(
        &self,
    ) -> Result<std::collections::BTreeMap<String, PluginId>, StoreError> {
        let path = self.paths.plugins_lock.clone();
        self.run(move |_| lock::read(&path)).await
    }

    async fn register_plugin(&self, name: &str, id: PluginId) -> Result<(), StoreError> {
        let path = self.paths.plugins_lock.clone();
        let name = name.to_owned();
        self.run(move |_| lock::register(&path, name, id)).await
    }

    async fn registry(&self) -> Result<RegistryState, StoreError> {
        self.run(|connection| registry::read(connection)).await
    }

    async fn insert_generation(
        &self,
        generation: &NewGeneration,
        activate: bool,
    ) -> Result<GenerationId, StoreError> {
        let generation = generation.clone();
        self.run(move |connection| registry::insert(connection, &generation, activate))
            .await
    }

    async fn activate(
        &self,
        plugin: PluginId,
        to: Option<GenerationId>,
        failed: &[GenerationId],
    ) -> Result<(), StoreError> {
        let failed = failed.to_vec();
        self.run(move |connection| registry::activate(connection, plugin, to, &failed))
            .await
    }

    async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, StoreError> {
        let hash = ContentHash::of(bytes);
        content::put(&self.blob_path(&hash), bytes).await?;
        Ok(hash)
    }

    async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        content::get(&self.blob_path(hash), hash, StoreError::Blob).await
    }

    async fn put_artifact(&self, bytes: &[u8]) -> Result<ContentHash, StoreError> {
        let hash = ContentHash::of(bytes);
        content::put(&self.artifact_path(&hash), bytes).await?;
        Ok(hash)
    }

    async fn artifact(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        content::get(&self.artifact_path(hash), hash, StoreError::Artifact).await
    }

    fn blob_path(&self, hash: &ContentHash) -> PathBuf {
        let address = hash.to_string();
        self.paths.blobs.join(&address[..2]).join(address)
    }
}

fn signed(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(backend)
}
