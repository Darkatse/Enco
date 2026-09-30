use async_trait::async_trait;
use enco_core::*;
use std::path::PathBuf;

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
    /// Look up a Session without creating it.
    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>;
    /// Create if absent, with the supplied time, local binding at epoch one and default requirements.
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
    /// The database requires a newer Enco binary.
    #[error("database was created by a newer Enco (schema version {0})")]
    NewerSchema(u32),
    /// The storage engine or filesystem operation failed.
    #[error("storage: {0}")]
    Backend(String),
}
