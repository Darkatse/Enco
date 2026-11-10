mod input;
mod lifecycle;
mod overview;
pub use lifecycle::{Halted, SessionLookupError};
pub use overview::{ContextUsage, LastRequest, ReplyModel, RunState, SessionOverview};

use crate::{
    session::{SessionDeps, SessionHandle, lock},
    snapshot::Snapshot,
    *,
};
use enco_core::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Ports assembled once by the application; no service locator is involved.
pub struct KernelDeps {
    /// Durable authority.
    pub store: Arc<dyn Store>,
    /// Sole owner of plugin generations and published exports.
    pub registry: Arc<Registry>,
    /// Absolute home location supplied to context policies.
    pub home: std::path::PathBuf,
    /// The owner instructions, read once per Round.
    pub owner_instructions: Arc<dyn ContextSource<Supply = Option<String>>>,
    /// The selected profile's instructions, read once per Round.
    pub profile_instructions: Arc<dyn ContextSource<Supply = Option<String>>>,
    /// The memories offered once per Round.
    pub memories: Arc<dyn ContextSource<Supply = OfferedMemories>>,
    /// Host capabilities exposed through the ordinary Tool port.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Clock supplied by the host or tests.
    pub clock: Arc<dyn Clock>,
}

/// Single-node execution owner. Sessions serialize their own Log writes.
pub struct Kernel {
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) deps: Arc<SessionDeps>,
    pub(crate) sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
}

/// Current node and Session execution state.
#[derive(serde::Serialize)]
pub struct Status {
    /// Owner's timezone from the selected configuration.
    pub timezone: Tz,
    /// Persistent node identity.
    pub node: NodeId,
    /// Policy observed at the next Round boundary.
    pub safe_mode: bool,
    /// Available profile names; endpoint credentials are never exposed.
    pub profiles: Vec<ProfileName>,
    /// State of every registered Session actor.
    pub sessions: Vec<SessionStatus>,
    /// Failure that stopped the Scheduler, if any.
    pub scheduler_stopped: Option<String>,
}

/// Live execution status, separate from durable Session facts.
#[derive(serde::Serialize)]
pub struct SessionStatus {
    /// Durable Session metadata.
    pub session: SessionRecord,
    /// Whether a Run is currently executing.
    pub running: bool,
    /// FIFO position while awaiting a child Run slot, starting at one.
    pub waiting: Option<usize>,
    /// Storage failure that stopped this actor, if any.
    pub stopped: Option<String>,
}

/// Whether the Session's actor is executing a Run now.
fn running(handles: &HashMap<SessionId, Arc<SessionHandle>>, session: SessionId) -> bool {
    handles
        .get(&session)
        .is_some_and(|handle| lock(&handle.run_cancel).is_some())
}

/// Public command failure; Round failures are recorded in the Log instead.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Schedules could not be read back into occurrences.
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    /// A registry operation failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Invalid composition-root input.
    #[error("configuration: {0}")]
    Config(String),
    /// New owners cannot be registered after shutdown starts.
    #[error("kernel is shutting down")]
    ShuttingDown,
    /// An owner task exited without completing its normal shutdown path.
    #[error("owner task failed: {0}")]
    TaskFailed(String),
    /// The actor stopped after an owner could not commit its state.
    #[error("session {0} is stopped: {1}")]
    SessionStopped(SessionId, String),
    /// The requested Session has no matching recorded Attempt.
    #[error("session {0} has no attempt {1}")]
    UnknownAttempt(SessionId, String),
    /// A persisted plan cannot be decoded or resolved against its recorded history.
    #[error("attempt {attempt}: invalid recorded plan: {reason}")]
    InvalidPlan {
        /// Attempt which references the invalid plan.
        attempt: AttemptId,
        /// Decoding or resolution failure.
        reason: String,
    },
    /// No such Session is registered.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// A name or ID did not resolve to a Session to act on.
    #[error(transparent)]
    Lookup(#[from] SessionLookupError),
    /// The requested profile is absent from the node configuration.
    #[error("unknown profile {0}")]
    UnknownProfile(ProfileName),
    /// The requested working directory is not an absolute UTF-8 path.
    #[error(transparent)]
    InvalidWorkingDir(#[from] InvalidWorkingDir),
    /// A backtrack must point at a position that this Session's conversation has recorded.
    #[error("session {session} cannot go back to {to:?}: {reason}")]
    Backtrack {
        /// The Session the owner addressed.
        session: SessionId,
        /// The position the owner pointed at.
        to: LogPos,
        /// Why the position cannot be a target.
        reason: &'static str,
    },
}

/// A delivery failure with the Session name resolved before presentation.
#[derive(serde::Serialize)]
pub struct DeliveryStatus {
    /// Original delivery and settlement facts.
    #[serde(flatten)]
    pub settlement: DeliverySettlement,
    /// The Session that produced the delivery.
    pub session: SessionRef,
}

impl Kernel {
    /// Bind capabilities and start unarchived Sessions, plus archived ones not yet quiescent.
    pub async fn start(mut deps: KernelDeps) -> Result<Self, KernelError> {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let subtrees = Arc::new(crate::subtree::Subtrees {
            store: deps.store.clone(),
            clock: deps.clock.clone(),
            sessions: sessions.clone(),
            deps: std::sync::OnceLock::new(),
        });
        deps.tools.extend(
            crate::builtin::tools(&subtrees)
                .map_err(|error| KernelError::Config(format!("tool declaration: {error}")))?,
        );
        let declared: std::collections::BTreeSet<_> =
            deps.registry.host_tool_names().iter().cloned().collect();
        let actual: std::collections::BTreeSet<_> =
            deps.tools.iter().map(|tool| tool.spec().name).collect();
        if declared != actual {
            return Err(KernelError::Config(format!(
                "host tool declarations differ from handlers: missing handlers {:?}; undeclared handlers {:?}",
                declared.difference(&actual).collect::<Vec<_>>(),
                actual.difference(&declared).collect::<Vec<_>>()
            )));
        }
        let node = deps.store.node().await?;
        let snapshot = Arc::new(Snapshot::new(node.id, &deps)?);
        let kernel = Self {
            scheduler: Mutex::new(None),
            deps: Arc::new(SessionDeps {
                home: deps.home,
                store: deps.store,
                clock: deps.clock,
                registry: deps.registry,
                snapshot,
                shutdown: CancellationToken::new(),
                subtrees,
                slots: Arc::new(crate::child_slots::ChildSlots::default()),
                scheduler: Arc::default(),
            }),
            sessions,
        };
        let _ = kernel.deps.subtrees.deps.set(Arc::downgrade(&kernel.deps));
        for session in kernel.deps.store.sessions().await? {
            // An archived Session whose halt still waits was stopped before it took effect.
            if session.archived_at.is_none()
                || !crate::subtree::quiescent(kernel.deps.store.as_ref(), session.id).await?
            {
                kernel.ensure_actor(session)?;
            }
        }
        *lock(&kernel.scheduler) = Some(crate::scheduler::start(
            kernel.deps.store.clone(),
            kernel.deps.clock.clone(),
            kernel.deps.registry.subscribe(),
            kernel.sessions.clone(),
            kernel.deps.scheduler.clone(),
            kernel.deps.shutdown.clone(),
        ));
        Ok(kernel)
    }

    fn ensure_actor(&self, session: SessionRecord) -> Result<Arc<SessionHandle>, KernelError> {
        self.deps
            .ensure_actor(session)
            .ok_or(KernelError::ShuttingDown)
    }

    fn handle(&self, id: SessionId) -> Result<Arc<SessionHandle>, KernelError> {
        let handle = lock(&self.sessions)
            .get(&id)
            .cloned()
            .ok_or(KernelError::UnknownSession(id))?;
        if let Some(reason) = lock(&handle.stopped).clone() {
            return Err(KernelError::SessionStopped(id, reason));
        }
        Ok(handle)
    }

    /// Check protocol update acceptance before preparing its content or routes.
    pub async fn connection_update_seen(
        &self,
        key: &str,
        update: &str,
    ) -> Result<bool, KernelError> {
        Ok(self.deps.store.connection_update_seen(key, update).await?)
    }

    /// Read a connection's last accepted state.
    pub async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, KernelError> {
        Ok(self.deps.store.connection(key).await?)
    }

    /// Read recent final failures, retaining their original Session and Log references.
    pub async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliveryStatus>, KernelError> {
        let failures = self.deps.store.delivery_failures(key).await?;
        let mut result = Vec::with_capacity(failures.len());
        for settlement in failures {
            let id = settlement.delivery.session;
            let session = self
                .deps
                .store
                .session(id)
                .await?
                .ok_or(StoreError::UnknownSession(id))?;
            result.push(DeliveryStatus {
                settlement,
                session: SessionRef {
                    id,
                    name: session.name,
                },
            });
        }
        Ok(result)
    }

    /// Find where a channel message with this protocol ID became a Session's input.
    ///
    /// Commands recorded as inputs, such as backtracks, never match.
    pub async fn consumed_message(
        &self,
        protocol: &str,
        account: &str,
        conversation: &str,
        message: &str,
    ) -> Result<Option<ConsumedMessage>, KernelError> {
        Ok(self
            .deps
            .store
            .consumed_message(protocol, account, conversation, message)
            .await?)
    }

    /// Find the connection's delivery to `target` that sent the protocol message `message`.
    pub async fn delivered_message(
        &self,
        key: &str,
        target: &str,
        message: &str,
    ) -> Result<Option<Delivery>, KernelError> {
        Ok(self
            .deps
            .store
            .delivered_message(key, target, message)
            .await?)
    }

    /// Subscribe to a Session's newly committed facts; the Log remains the durable source.
    ///
    /// The subscription ends once the Session is archived and has stopped, so for such a Session
    /// it ends at once. An unarchived Session's actor starts here if it has none yet, such as a
    /// child whose parent's commit is still waking it.
    pub async fn subscribe(
        &self,
        session: SessionId,
    ) -> Result<broadcast::Receiver<Entry>, KernelError> {
        let handle = match self.handle(session) {
            Err(KernelError::UnknownSession(_)) => {
                let record = self.session(session).await?;
                if record.archived_at.is_some() {
                    let (_, ended) = broadcast::channel(1);
                    return Ok(ended);
                }
                self.ensure_actor(record)?
            }
            handle => handle?,
        };
        Ok(handle.entries.subscribe())
    }

    /// Signal the current Run; its actor settles work before ending.
    pub fn cancel(&self, session: SessionId) -> Result<bool, KernelError> {
        let handle = self.handle(session)?;
        let current = lock(&handle.run_cancel);
        if let Some(token) = current.as_ref() {
            token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Change the policy observed at the next Round boundary.
    pub async fn set_safe_mode(&self, enabled: bool) -> Result<(), KernelError> {
        Ok(self.deps.store.set_safe_mode(enabled).await?)
    }

    /// A Session's live execution status from its actor and the child slot queue.
    fn session_status(
        &self,
        handles: &HashMap<SessionId, Arc<SessionHandle>>,
        session: SessionRecord,
    ) -> SessionStatus {
        SessionStatus {
            waiting: self.deps.slots.position(session.id),
            running: running(handles, session.id),
            stopped: handles
                .get(&session.id)
                .and_then(|h| lock(&h.stopped).clone()),
            session,
        }
    }

    /// Read durable Session metadata.
    pub async fn sessions(&self) -> Result<Vec<SessionRecord>, KernelError> {
        Ok(self.deps.store.sessions().await?)
    }

    /// Read immutable facts in structural order.
    pub async fn log(
        &self,
        session: SessionId,
        after: Option<LogPos>,
    ) -> Result<Vec<Entry>, KernelError> {
        Ok(self.deps.store.log(session, after).await?)
    }

    /// The frozen plan an Attempt recorded at `hash`, including the tools it disclosed.
    pub async fn plan(&self, hash: &ContentHash) -> Result<ContextPlan, KernelError> {
        Ok(self.deps.store.plan(hash).await?)
    }

    /// Plugin commands share the registry used by completion and embedding callers.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.deps.registry
    }

    /// Signal cancellation and wait for every owner to stop.
    pub async fn shutdown(&self) -> Result<(), KernelError> {
        self.deps.shutdown.cancel();
        let handles: Vec<_> = lock(&self.sessions).values().cloned().collect();
        // Await every owner even if an earlier join fails.
        let mut failure = None;
        for handle in handles {
            let task = lock(&handle.task).take();
            if let Some(task) = task
                && let Err(error) = task.await
            {
                failure = Some(KernelError::TaskFailed(format!(
                    "Session {}: {error}",
                    handle.id
                )));
            }
        }
        let scheduler = lock(&self.scheduler).take();
        if let Some(task) = scheduler
            && let Err(error) = task.await
        {
            failure = Some(KernelError::TaskFailed(format!("Scheduler: {error}")));
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.deps.shutdown.cancel();
    }
}
