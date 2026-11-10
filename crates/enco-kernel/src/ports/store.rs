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

    // ---- Sessions (each row folds its creation record, archive and own Log)
    /// List existing Sessions in their effective view.
    async fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError>;
    /// Read a Session's effective view: its committed row with its pending commands applied, read
    /// in one transaction.
    ///
    /// Readers outside the Session see it; the Session's actor and Rounds read the committed view.
    async fn session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError>;
    /// Read a Session's committed view, the row its own Log and recorded facts give; the actor
    /// reads it after consuming the commands acting on the Session.
    async fn committed_session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError>;
    /// Read a Session's committed view and its ancestors', nearest first, in one transaction.
    ///
    /// It is what a Round samples, so its permissions narrow along one state of the tree.
    async fn committed_lineage(
        &self,
        id: SessionId,
    ) -> Result<(SessionRecord, Vec<SessionRecord>), StoreError>;
    /// Every unarchived Session with each view its Rounds may read from now on, read in one
    /// transaction.
    async fn unarchived_views(&self) -> Result<Vec<SessionViews>, StoreError>;
    /// Look up an unarchived Session by name, in its effective view, without creating it.
    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>;
    /// The unarchived Sessions this one delegated to, in creation order and effective view.
    async fn children(&self, parent: SessionId) -> Result<Vec<SessionRecord>, StoreError>;
    /// Create a root through the root directory if no unarchived Session has the name.
    ///
    /// It runs here at epoch one, with the default profile and this node's default working
    /// directory.
    async fn ensure_session(
        &self,
        name: &SessionName,
        created_at: DateTime<Utc>,
    ) -> Result<SessionRecord, StoreError>;

    /// Atomically halt the unarchived subtree, optionally archiving it; returns it root first.
    ///
    /// The root receives `source`; descendants receive `Parent`. Each input carries `halt`. An
    /// archive records one control-plane fact for the root, from which its descendants are
    /// archived too, and releases their names at once. A root that is already archived writes
    /// nothing and returns no Sessions.
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

    // ---- Schedules (each row folds its Session's own facts)
    /// The schedules of every unarchived Session, or of one, in their effective view.
    ///
    /// That is the committed rows with the owners' pending cancels and the pending reminders
    /// applied.
    async fn schedules(&self, session: Option<SessionId>) -> Result<Vec<Schedule>, StoreError>;

    // ---- Identity, generations and wiring (writer: Registry)
    /// Read accepted intent and this node's references, restored at startup.
    async fn registry(&self) -> Result<RegistryState, StoreError>;

    /// Atomically accept one registration, its content and this node's references.
    async fn register(&self, write: &RegistrationWrite) -> Result<(), StoreError>;
    /// Record that a trial generation is healthy on this node, keeping its rollback target.
    async fn promote(&self, generation: GenerationId) -> Result<(), StoreError>;
    /// Atomically move this node's references and record attributable failures and a notice.
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

/// A root Session requested together with the input that first reaches it.
///
/// The root directory, the authority over root names, gives an occupied name a numeric suffix
/// and records the creation; the Session starts here at epoch one in this node's default
/// working directory.
#[derive(Clone)]
pub struct NewSession {
    /// Identity chosen by the creator, which also records it in its own state.
    pub id: SessionId,
    /// Requested root name.
    pub name: SessionName,
    /// Registered profile the Session starts with.
    pub profile: ProfileName,
    /// Creation time for display.
    pub created_at: DateTime<Utc>,
}

/// One Session's views, from the one its Rounds read now to its effective view.
#[derive(Debug, Clone)]
pub struct SessionViews {
    /// The row its own Log and recorded facts give, which its Rounds and its descendants' Rounds
    /// read.
    pub committed: SessionRecord,
    /// The committed row after each pending Event in Inbox order, the last being the effective
    /// view; empty when nothing is pending.
    ///
    /// The actor consumes the commands acting on the Session in Inbox order, a prefix at a time,
    /// so any of these may become its committed view.
    pub ahead: Vec<SessionRecord>,
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
    /// It names a schedule that is not active in the target Session's effective view.
    ///
    /// That is a reminder or an owner's cancel of a schedule that is cancelled, of a one-time
    /// schedule that has reminded already, or of none the Session created.
    ///
    /// The commands and reminders still in the Inbox count.
    Inactive,
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
    /// The target Session does not exist.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// A Session's facts name a schedule it never created.
    #[error("session {session} has no schedule {schedule}")]
    UnknownSchedule {
        /// Session whose commit names the schedule.
        session: SessionId,
        /// Schedule it never created.
        schedule: ScheduleId,
    },
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
    /// Upgrading the database from the previous version failed; it is left as it was.
    #[error("upgrading the database from version {from}: {reason}")]
    Migration {
        /// The version the database has.
        from: u32,
        /// What failed, and where the copy taken before the upgrade is.
        reason: String,
    },
    /// The storage engine or filesystem operation failed.
    #[error("storage: {0}")]
    Backend(String),
}
