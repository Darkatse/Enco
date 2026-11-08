use async_trait::async_trait;
use enco_core::*;
use std::path::PathBuf;
mod commit;
mod registry;
pub use commit::*;
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
    /// Look up an unarchived Session by name without creating it.
    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>;
    /// The unarchived Sessions this one delegated to, in creation order.
    async fn children(&self, parent: SessionId) -> Result<Vec<SessionRecord>, StoreError>;
    /// Create if absent, with local binding at epoch one, the default profile and this node's
    /// default working directory.
    async fn ensure_session(
        &self,
        name: &SessionName,
        created_at: DateTime<Utc>,
    ) -> Result<SessionRecord, StoreError>;

    /// Persist the working directory for subsequent Rounds, without checking availability.
    async fn set_working_dir(
        &self,
        id: SessionId,
        working_dir: &WorkingDir,
    ) -> Result<(), StoreError>;

    /// Atomically halt the unarchived subtree, optionally archiving it; returns it root first.
    ///
    /// The root receives `source`; descendants receive `Parent`. Each input carries `halt`. A root
    /// that is already archived writes nothing and returns no Sessions.
    async fn halt_subtree(
        &self,
        root: SessionId,
        source: EventSource,
        halt: Halt,
        at: DateTime<Utc>,
        archive: bool,
    ) -> Result<Vec<SessionRef>, StoreError>;

    // ---- Log (writer: the owning Session actor)
    /// Read immutable facts in structural order after an optional position.
    async fn log(
        &self,
        session: SessionId,
        after: Option<LogPos>,
    ) -> Result<Vec<Entry>, StoreError>;
    /// Read at most `limit` entries immediately before `before`, or before the end without it, in
    /// structural order; fewer means the read reached the start of the Log.
    async fn log_before(
        &self,
        session: SessionId,
        before: Option<LogPos>,
        limit: usize,
    ) -> Result<Vec<Entry>, StoreError>;
    /// Read the entry at `pos`, if the Log has one there.
    async fn log_entry(&self, session: SessionId, pos: LogPos)
    -> Result<Option<Entry>, StoreError>;
    /// Fold a child's unconsumed inputs, then its Log, newest first through `ChildStateFold`,
    /// reading only as far back as the fold needs.
    async fn child_state(&self, child: SessionId) -> Result<ChildState, StoreError>;
    /// Atomically append facts and consume their Inbox inputs after checking ownership and positions.
    async fn commit(&self, session: SessionId, commit: Commit)
    -> Result<CommitOutcome, StoreError>;
    /// Restore a recorded request plan and verify its complete content address.
    async fn plan(&self, hash: &ContentHash) -> Result<ContextPlan, StoreError>;
    /// Find where a channel message with this protocol ID was consumed.
    ///
    /// Commands recorded as inputs, such as backtracks, never match.
    async fn consumed_message(
        &self,
        protocol: &str,
        account: &str,
        conversation: &str,
        message: &str,
    ) -> Result<Option<ConsumedMessage>, StoreError>;

    // ---- Inbox (any producer may submit; only the Session actor consumes through Commit)
    /// Atomically accept Events with a connection's state, and with it the protocol update, the
    /// Session that update creates and a delivery outcome, if any.
    ///
    /// Accepting an update key twice is an error: the connection checks it before preparing.
    async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Acceptance, StoreError>;
    /// Check durable update acceptance before preparing attachments or creating Sessions.
    async fn connection_update_seen(&self, key: &str, update: &str) -> Result<bool, StoreError>;
    /// Read the state owned by one channel connection.
    async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, StoreError>;
    /// Read recent failed or uncertain deliveries in reverse commit order.
    async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, StoreError>;
    /// Find the connection's delivery to `target` whose confirmed parts include the protocol
    /// message `message`.
    async fn delivered_message(
        &self,
        key: &str,
        target: &str,
        message: &str,
    ) -> Result<Option<Delivery>, StoreError>;
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
    ///
    /// Mark it done when no occurrence remains; archived targets return `Archived` without writes.
    async fn fire_schedule(
        &self,
        expected_last: Option<DateTime<Utc>>,
        done: bool,
        event: &Event,
    ) -> Result<Accepted, StoreError>;

    // ---- Identity, generations and wiring (writer: Registry)
    /// Select the profile under the registry committer, effective at the next Round.
    async fn set_profile(&self, id: SessionId, profile: &ProfileName) -> Result<(), StoreError>;
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
    /// Persist content the Agent or the owner may read by path, such as an attachment, an image
    /// or a long tool result, before recording its content address.
    async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>;
    /// Read bytes and verify they match their recorded address.
    async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;
    /// Local path of a blob, which the Agent reads with its tools; the file holds the bytes as
    /// written.
    fn blob_path(&self, hash: &ContentHash) -> PathBuf;

    // ---- Evidence (content-addressed, idempotent writes)
    /// Persist evidence of a mapper call, such as its record or a response it read, addressed by
    /// its bytes; nothing reads it by path.
    async fn put_evidence(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>;
    /// Read evidence and verify it matches its address.
    async fn evidence(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;
}

/// Persistent node identity and recovery policy.
#[derive(Debug, Clone)]
pub struct NodeRecord {
    /// Persistent node identity.
    pub id: NodeId,
    /// Whether the next Round uses only the lifeline.
    pub safe_mode: bool,
}

/// An immutable request plan whose complete content address is computed once before committing.
pub struct PlanWrite {
    plan: ContextPlan,
    hash: ContentHash,
}

impl PlanWrite {
    /// Prepare a plan for atomic acceptance with its Attempt, without changing storage.
    pub fn new(plan: ContextPlan) -> Result<Self, StoreError> {
        let hash = plan
            .hash()
            .map_err(|error| StoreError::Backend(error.to_string()))?;
        Ok(Self { plan, hash })
    }

    /// The address of the complete plan, independent of its storage representation.
    pub fn hash(&self) -> ContentHash {
        self.hash
    }

    /// The exact content accepted with the Attempt and shared with the request sender.
    pub fn plan(&self) -> &ContextPlan {
        &self.plan
    }
}

/// State returned to the connection owner for atomic acceptance.
#[derive(Clone)]
pub struct ConnectionWrite {
    /// The connection-state key, `<protocol>:<account>`.
    pub key: String,
    /// Complete state snapshot owned by this connection.
    pub state: serde_json::Value,
    /// Protocol update accepted with this state, absent for outbound or cursor-only progress.
    pub update: Option<String>,
    /// The Session this update creates, such as a group's first, never adopting an existing one.
    pub session: Option<NewSession>,
    /// Final settlement of a Log-derived delivery, if any.
    pub settlement: Option<DeliverySettlement>,
}

/// A Session created together with the input that first reaches it.
///
/// It starts bound to this node at epoch one; its origin determines its directory and name policy.
#[derive(Clone)]
pub struct NewSession {
    /// Identity chosen by the creator, which also records it in its own state.
    pub id: SessionId,
    /// Full validated name; its origin determines how occupied names are handled.
    pub name: SessionName,
    /// Registered profile the Session starts with.
    pub profile: ProfileName,
    /// Root or child creation facts.
    pub origin: NewSessionOrigin,
    /// Creation time for display.
    pub created_at: DateTime<Utc>,
}

/// Origin fixed when a Session is created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewSessionOrigin {
    /// A root uses the node's default directory; an occupied name gets a numeric suffix.
    Root,
    /// A child uses its parent's identity and an explicitly resolved working directory.
    Child {
        /// Fixed parent identity.
        parent: SessionId,
        /// Directory supplied by the parent, without availability checks.
        working_dir: WorkingDir,
    },
}

/// What an atomic acceptance committed.
pub struct Acceptance {
    /// Event results in input order.
    pub events: Vec<Accepted>,
    /// The Session created by this acceptance, as stored.
    pub session: Option<SessionRecord>,
}

/// Where a channel message became a Session's input.
pub struct ConsumedMessage {
    /// The Session that consumed it.
    pub session: SessionId,
    /// Its position in that Session's Log.
    pub pos: LogPos,
    /// Whether its sender was the owner when it was accepted.
    pub owner: bool,
}

/// Whether an Event was newly accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    /// Accepted for the first time.
    New,
    /// The Event ID was already accepted.
    Duplicate,
    /// The target Session is archived and accepts no input.
    Archived,
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
    /// Recorded evidence is missing or does not match its address.
    #[error("evidence {0} is corrupt or missing")]
    Evidence(ContentHash),
    /// A plan or one of its storage dependencies is missing or fails content verification.
    #[error("plan {hash}: {reason}")]
    Plan {
        /// Address of the complete plan being restored.
        hash: ContentHash,
        /// The missing dependency or invalid stored content.
        reason: String,
    },
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
