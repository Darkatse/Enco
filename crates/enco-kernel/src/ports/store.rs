use async_trait::async_trait;
use enco_core::*;
use std::path::PathBuf;
mod registry;
pub use registry::*;

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
    /// Scheduler-owned insertion of a new schedule, stored as given.
    async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>;
    /// Scheduler-owned cancellation; return whether an active schedule changed.
    async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError>;
    /// Read schedules, optionally filtered by state.
    async fn schedules(&self, state: Option<ScheduleState>) -> Result<Vec<Schedule>, StoreError>;
    /// Advance an active Schedule from expected_last and accept its Event atomically.
    /// Mark it done when no occurrence remains.
    async fn fire_schedule(
        &self,
        expected_last: Option<DateTime<Utc>>,
        done: bool,
        event: &Event,
    ) -> Result<(), StoreError>;

    // ---- Identity, generations and wiring (writer: Registry)
    /// Select the profile under the registry committer, effective at the next Round.
    async fn set_profile(&self, id: SessionId, profile: &str) -> Result<(), StoreError>;
    /// Read registration history and the references restored at startup.
    async fn registry(&self) -> Result<RegistryState, StoreError>;
    /// Record embedded factory content without activating it or creating a registration.
    async fn record_factories(&self, generations: &[GenerationRecord]) -> Result<(), StoreError>;

    /// Atomically accept one registration, its content and its runtime references.
    async fn register(&self, write: &RegistrationWrite) -> Result<(), StoreError>;
    /// Promote an observed trial without changing its identity or rollback target.
    async fn promote(&self, generation: GenerationId) -> Result<(), StoreError>;
    /// Atomically move runtime references and record attributable failures and a notice.
    async fn activate(&self, change: &ActivationWrite) -> Result<(), StoreError>;
    /// Remove component files which no runtime reference retains.
    async fn retain_artifacts(
        &self,
        hashes: &std::collections::HashSet<ContentHash>,
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
    /// Local path of a blob, recorded with the truncation fact of a long tool result.
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
    /// The scheduler's expected state no longer matches the authority.
    #[error("schedule {0} is not active at the expected occurrence")]
    ScheduleNotActive(ScheduleId),
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
    /// The authority uses another schema version; opening it never deletes or migrates data.
    #[error(
        "database schema version {found} does not match this Enco ({expected}); {}",
        if .found > .expected { "upgrade Enco" } else { "migrate the database by hand or recreate it" }
    )]
    SchemaVersion {
        /// Version recorded in the database.
        found: u32,
        /// Version understood by this binary.
        expected: u32,
    },
    /// The storage engine or filesystem operation failed.
    #[error("storage: {0}")]
    Backend(String),
}
