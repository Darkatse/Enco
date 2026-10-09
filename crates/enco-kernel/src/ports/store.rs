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
    /// Promote a trial generation after successful observation.
    async fn promote(&self, generation: GenerationId) -> Result<(), StoreError>;
    /// Commit each failed generation with its reason, routing and an optional notice atomically.
    async fn activate(
        &self,
        plugin: PluginId,
        to: Option<GenerationId>,
        failed: &[(GenerationId, Failure)],
        event: Option<&Event>,
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

/// Durable registry facts; compiled exports are a derived view.
pub struct RegistryState {
    /// All generation records in commit order.
    pub generations: Vec<GenerationRecord>,
    /// Active generations; a missing plugin has no active generation.
    pub active: BTreeMap<PluginId, GenerationId>,
}

/// A generation awaiting its registry-assigned sequence number.
#[derive(Clone)]
pub struct NewGeneration {
    /// Plugin identity which owns this history.
    pub plugin: PluginId,
    /// Component content address.
    pub artifact: ContentHash,
    /// Configuration of this generation, not model invocation settings.
    pub config: serde_json::Value,
    /// Factory registration or explicit deployment.
    pub origin: Origin,
    /// Initial eligibility: trial or healthy; failures are committed through activate.
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
            failure: None,
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
    /// Plugin identity file could not be read, interpreted or atomically replaced.
    #[error("plugins.lock: {0}")]
    Lock(String),
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
