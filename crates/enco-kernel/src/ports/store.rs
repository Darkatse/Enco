use async_trait::async_trait;
use enco_core::*;
use std::{collections::BTreeMap, path::PathBuf};

/// Persistence boundary; only the owning actor may commit its state.
#[async_trait]
pub trait Store: Send + Sync {
    // ---- Node state (writer: Kernel)
    /// Read persistent node identity and safe-mode state.
    async fn node(&self) -> Result<NodeRecord, StoreError>;
    /// Kernel-owned update, observed at the next Round boundary.
    async fn set_safe_mode(&self, enabled: bool) -> Result<(), StoreError>;

    // ---- Sessions (writer: Kernel)
    /// List existing Sessions.
    async fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError>;
    /// Read the metadata sampled by the Session actor at each Round boundary.
    async fn session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError>;
    /// Select the profile name to use from the next Round onward.
    async fn set_profile(&self, id: SessionId, profile: &str) -> Result<(), StoreError>;
    /// Look up a Session without creating it.
    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>;
    /// Create if absent, with local binding at epoch one and the default profile.
    async fn ensure_session(
        &self,
        name: &str,
        created_at: DateTime<Utc>,
    ) -> Result<SessionRecord, StoreError>;

    // ---- Log (writer: the owning Session actor)
    /// Read immutable facts in structural order after an optional position.
    async fn log(
        &self,
        session: SessionId,
        after: Option<LogPos>,
    ) -> Result<Vec<Entry>, StoreError>;
    /// Atomically append facts and consume their Inbox inputs after checking ownership and positions.
    async fn commit(&self, session: SessionId, commit: Commit) -> Result<(), StoreError>;

    // ---- Inbox (any producer may submit; only the Session actor consumes through Commit)
    /// Atomically accept Events, connection state and an optional final delivery outcome.
    async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Vec<Accepted>, StoreError>;
    /// Read the state owned by one channel connection.
    async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, StoreError>;
    /// Read recent failed or uncertain deliveries in reverse commit order.
    async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, StoreError>;
    /// Read unconsumed inputs in acceptance order.
    async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError>;

    // ---- Schedules (writer: Scheduler actor)
    /// Scheduler-owned insertion of a pending reminder.
    async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>;
    /// Scheduler-owned cancellation; return whether a pending reminder changed.
    async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError>;
    /// Read reminders, optionally filtered by state.
    async fn schedules(
        &self,
        state: Option<ScheduleStateKind>,
    ) -> Result<Vec<Schedule>, StoreError>;
    /// Mark a pending Schedule fired and accept its Event into the Inbox in one transaction.
    async fn fire_schedule(&self, id: ScheduleId, event: &Event) -> Result<(), StoreError>;

    // ---- Plugin identity and generations (writer: Registry)
    /// Read the authoritative name-to-identity mapping from plugins.lock.
    async fn plugin_names(&self) -> Result<BTreeMap<String, PluginId>, StoreError>;
    /// Register a new name before any generation is committed for its identity.
    async fn register_plugin(&self, name: &str, id: PluginId) -> Result<(), StoreError>;
    /// Read all durable generations and active routing.
    async fn registry(&self) -> Result<RegistryState, StoreError>;
    /// Assign a generation number and optionally activate it in the same transaction.
    async fn insert_generation(
        &self,
        generation: &NewGeneration,
        activate: bool,
    ) -> Result<GenerationId, StoreError>;
    /// Commit failed candidates and the resulting active generation atomically.
    async fn activate(
        &self,
        plugin: PluginId,
        to: Option<GenerationId>,
        failed: &[GenerationId],
    ) -> Result<(), StoreError>;
    /// Persist immutable component bytes separately from removable history blobs.
    async fn put_artifact(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>;
    /// Read and verify component bytes by content address.
    async fn artifact(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;

    // ---- Blobs (content-addressed, idempotent writes)
    /// Persist immutable bytes before recording their content address.
    async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>;
    /// Read bytes and verify they match their recorded address.
    async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;
    /// Local path of immutable content, allowing the model to inspect a long result with fs_read.
    fn blob_path(&self, hash: &ContentHash) -> PathBuf;
}

/// Persistent node identity and recovery policy.
#[derive(Debug, Clone)]
pub struct NodeRecord {
    /// Persistent node identity.
    pub id: NodeId,
    /// Whether the next Round uses only the lifeline.
    pub safe_mode: bool,
}

/// Durable registry facts; compiled exports are a derived view.
pub struct RegistryState {
    /// All generation records in commit order.
    pub generations: Vec<GenerationRecord>,
    /// The active generation, if any, for each registered plugin row.
    pub active: BTreeMap<PluginId, Option<GenerationId>>,
}

/// A generation awaiting its registry-assigned sequence number.
#[derive(Clone)]
pub struct NewGeneration {
    /// Plugin identity which owns this history.
    pub plugin: PluginId,
    /// Component content address.
    pub artifact: ContentHash,
    /// Configuration of this activation, not model invocation settings.
    pub config: serde_json::Value,
    /// Factory registration or explicit deployment.
    pub origin: Origin,
    /// Initial activation eligibility.
    pub status: GenerationStatus,
    /// Observation time supplied by the registry clock.
    pub created_at: DateTime<Utc>,
}

impl NewGeneration {
    /// Attach the number allocated by the durable commit.
    pub fn numbered(self, id: GenerationId) -> GenerationRecord {
        GenerationRecord {
            id,
            plugin: self.plugin,
            artifact: self.artifact,
            config: self.config,
            origin: self.origin,
            status: self.status,
            created_at: self.created_at,
        }
    }
}

/// An atomic append of Log facts and consumption of their corresponding Inbox rows.
pub struct Commit {
    /// Ordered facts to append.
    pub entries: Vec<Entry>,
    /// Inbox IDs consumed by the corresponding EventConsumed facts.
    pub consumed: Vec<EventId>,
}

/// State returned to the connection owner for atomic acceptance.
#[derive(Clone)]
pub struct ConnectionWrite {
    /// Adapter-declared protocol and account identity.
    pub key: String,
    /// Complete state snapshot owned by this connection.
    pub state: serde_json::Value,
    /// Final settlement of a Log-derived delivery, if any.
    pub settlement: Option<DeliverySettlement>,
}

/// Whether an Event was newly accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    /// Accepted for the first time.
    New,
    /// The Event ID was already accepted.
    Duplicate,
}

/// Filter for durable reminder state.
#[derive(Debug, Clone, Copy)]
pub enum ScheduleStateKind {
    /// Waiting for its due time.
    Pending,
    /// Already accepted into an Inbox.
    Fired,
    /// Cancelled before firing.
    Cancelled,
}

/// A failed storage operation or commit precondition.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A Commit must append at least one fact.
    #[error("commit must contain at least one entry")]
    EmptyCommit,
    /// The caller does not own this Session generation.
    #[error("session {session} is fenced: binding is {binding:?}, commit epoch is {epoch:?}")]
    Fenced {
        /// Session being committed.
        session: SessionId,
        /// Current execution owner.
        binding: Binding,
        /// Generation supplied by the writer.
        epoch: Epoch,
    },
    /// The append would leave a gap or overwrite a fact.
    #[error("session {session}: expected next position {expected:?}, got {got:?}")]
    OutOfOrder {
        /// Session being committed.
        session: SessionId,
        /// Required next Log position.
        expected: LogPos,
        /// Position supplied by the writer.
        got: LogPos,
    },
    /// Input consumption does not match accepted facts.
    #[error("inbox: {0}")]
    Inbox(String),
    /// This reminder has already fired or been cancelled.
    #[error("schedule {0} is not pending")]
    ScheduleNotPending(ScheduleId),
    /// The target Session does not exist.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// Immutable bytes are missing or do not match their address.
    #[error("blob {0} is corrupt or missing")]
    Blob(ContentHash),
    /// Component bytes are missing or do not match their address.
    #[error("artifact {0} is corrupt or missing")]
    Artifact(ContentHash),
    /// A referenced registry record does not exist.
    #[error("unknown generation {0}")]
    UnknownGeneration(GenerationId),
    /// Plugin identity file could not be read, interpreted or atomically replaced.
    #[error("plugins.lock: {0}")]
    Lock(String),
    /// The database requires a newer Enco binary.
    #[error("database was created by a newer Enco (schema version {0})")]
    NewerSchema(u32),
    /// The storage engine or filesystem operation failed.
    #[error("storage: {0}")]
    Backend(String),
}
