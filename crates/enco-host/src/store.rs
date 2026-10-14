mod accept;
mod commit;
mod connections;
mod content;
mod lock;
mod registry;
mod rows;
mod schedules;
mod sessions;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use rusqlite::{Connection, TransactionBehavior};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// `PRAGMA user_version` covers SQLite and its referenced plans. Until P4, persistent
/// format changes bump it and existing databases are migrated by hand.
const SCHEMA_VERSION: u32 = 7;

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
    match crate::sqlite::schema_version(&connection).map_err(backend)? {
        None => {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(backend)?;
            tx.execute_batch(include_str!("store/schema.sql"))
                .map_err(backend)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(backend)?;
            tx.execute(
                "INSERT INTO meta(key,value) VALUES ('node_id',?),('safe_mode','0')",
                [NodeId::new().to_string()],
            )
            .map_err(backend)?;
            tx.commit().map_err(backend)?;
        }
        Some(SCHEMA_VERSION) => {}
        Some(found) => {
            return Err(StoreError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            });
        }
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
        self.run(|connection| sessions::all(connection)).await
    }

    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError> {
        let name = name.to_owned();
        self.run(move |connection| sessions::named(connection, &name))
            .await
    }

    async fn session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError> {
        self.run(move |connection| sessions::get(connection, id))
            .await
    }

    async fn set_profile(&self, id: SessionId, profile: &str) -> Result<(), StoreError> {
        let profile = profile.to_owned();
        self.run(move |connection| sessions::set_profile(connection, id, &profile))
            .await
    }

    async fn ensure_session(
        &self,
        name: &str,
        created_at: DateTime<Utc>,
    ) -> Result<SessionRecord, StoreError> {
        let name = name.to_owned();
        let node = self.node;
        self.run(move |connection| sessions::ensure(connection, node, &name, created_at))
            .await
    }

    async fn log(
        &self,
        session: SessionId,
        after: Option<LogPos>,
    ) -> Result<Vec<Entry>, StoreError> {
        self.run(move |connection| sessions::log(connection, session, after))
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
        self.run(move |connection| connections::state(connection, &key))
            .await
    }

    async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, StoreError> {
        let key = key.to_owned();
        self.run(move |connection| connections::recent_failures(connection, &key))
            .await
    }

    async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError> {
        self.run(move |connection| sessions::pending(connection, session))
            .await
    }

    async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError> {
        let schedule = schedule.clone();
        self.run(move |connection| schedules::insert(connection, &schedule))
            .await
    }

    async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError> {
        self.run(move |connection| schedules::cancel(connection, id))
            .await
    }

    async fn schedules(&self, state: Option<ScheduleState>) -> Result<Vec<Schedule>, StoreError> {
        self.run(move |connection| schedules::list(connection, state))
            .await
    }

    async fn fire_schedule(
        &self,
        expected_last: Option<DateTime<Utc>>,
        done: bool,
        event: &Event,
    ) -> Result<(), StoreError> {
        let event = event.clone();
        self.run(move |connection| schedules::fire(connection, expected_last, done, &event))
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

    async fn insert_config_generation(
        &self,
        document: &serde_json::Value,
        created_at: DateTime<Utc>,
        plugins: &[NewGeneration],
    ) -> Result<(GenerationId, Vec<GenerationId>), StoreError> {
        let document = document.clone();
        let plugins = plugins.to_vec();
        self.run(move |connection| {
            registry::insert_config_generation(connection, &document, created_at, &plugins)
        })
        .await
    }

    async fn activate_config(&self, generation: GenerationId) -> Result<(), StoreError> {
        self.run(move |connection| registry::activate_config(connection, generation))
            .await
    }

    async fn promote(&self, generation: GenerationId) -> Result<(), StoreError> {
        self.run(move |connection| registry::promote(connection, generation))
            .await
    }

    async fn activate(
        &self,
        plugin: PluginId,
        to: Option<GenerationId>,
        failed: &[(GenerationId, Failure)],
        event: Option<&Event>,
    ) -> Result<(), StoreError> {
        let failed = failed.to_vec();
        let event = event.cloned();
        self.run(move |connection| {
            registry::activate(connection, plugin, to, &failed, event.as_ref())
        })
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
