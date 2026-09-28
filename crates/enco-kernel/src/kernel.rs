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
    /// Completion adapter.
    pub provider: Arc<dyn Provider>,
    /// Pure context policy.
    pub composer: Arc<dyn Composer>,
    /// Context sources in contribution order.
    pub context: Vec<Arc<dyn ContextSource>>,
    /// Host capabilities exposed through the ordinary Tool port.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Tool names required by Sessions with requires_lifeline.
    pub lifeline: Vec<String>,
    /// Clock supplied by the host or tests.
    pub clock: Arc<dyn Clock>,
}

/// Execution limits and the owner's local clock offset.
#[derive(Clone)]
pub struct KernelConfig {
    /// Model context and output limits.
    pub(crate) budget: Budget,
    /// Maximum Rounds in one activation.
    pub(crate) max_rounds_per_run: u32,
    /// Offset sampled by the application, without OS access in the kernel.
    pub(crate) utc_offset: chrono::FixedOffset,
}

impl KernelConfig {
    /// Validate execution budgets before any owner or persistent resource is started.
    pub fn new(
        budget: Budget,
        max_rounds_per_run: u32,
        utc_offset: chrono::FixedOffset,
    ) -> Result<Self, KernelError> {
        if max_rounds_per_run == 0
            || budget.context_tokens == 0
            || budget.max_output_tokens == 0
            || budget.max_output_tokens >= budget.context_tokens
        {
            return Err(KernelError::Config(
                "Round and token budgets must be positive, with output smaller than the window"
                    .into(),
            ));
        }
        Ok(Self {
            budget,
            max_rounds_per_run,
            utc_offset,
        })
    }
}

/// Single-node execution owner. Sessions serialize their own Log writes.
pub struct Kernel {
    schedules: Schedules,
    scheduler: Mutex<Option<tokio::task::JoinHandle<Result<(), ScheduleError>>>>,
    pub(crate) deps: Arc<SessionDeps>,
    pub(crate) sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
}

/// Current node and Session execution state.
#[derive(serde::Serialize)]
pub struct Status {
    /// Persistent node identity.
    pub node: NodeId,
    /// Policy observed at the next Round boundary.
    pub safe_mode: bool,
    /// Configured provider code.
    pub provider: CodeRef,
    /// Configured composer code.
    pub composer: CodeRef,
    /// State of every registered Session actor.
    pub sessions: Vec<SessionStatus>,
}

/// Live execution status, separate from durable Session facts.
#[derive(serde::Serialize)]
pub struct SessionStatus {
    /// Durable Session metadata.
    pub session: SessionRecord,
    /// Whether an activation is currently running.
    pub running: bool,
    /// Storage failure that stopped this actor, if any.
    pub stopped: Option<String>,
}

/// Public command failure; Round failures are recorded in the Log instead.
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// A durable operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The reminder owner failed.
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    /// Invalid composition-root input.
    #[error("configuration: {0}")]
    Config(String),
    /// New owners cannot be registered after shutdown starts.
    #[error("kernel is shutting down")]
    ShuttingDown,
    /// An owner task exited without completing its normal shutdown path.
    #[error("owner task failed: {0}")]
    TaskFailed(String),
    /// The actor stopped after a storage failure.
    #[error("session {0} is stopped: {1}")]
    SessionStopped(SessionId, String),
    /// No such Session is registered.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
}

impl Kernel {
    /// Bind capabilities and start one actor for each existing Session.
    pub async fn start(mut deps: KernelDeps, config: KernelConfig) -> Result<Self, KernelError> {
        let (schedules, scheduler) = crate::scheduler::Scheduler::channel();
        deps.tools.extend(crate::builtin::tools(schedules.clone()));
        let node = deps.store.node().await?;
        let snapshot = Arc::new(Snapshot::new(node.id, &deps)?);
        let kernel = Self {
            schedules,
            scheduler: Mutex::new(None),
            deps: Arc::new(SessionDeps {
                store: deps.store,
                clock: deps.clock,
                snapshot,
                config,
                shutdown: CancellationToken::new(),
            }),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        };
        for session in kernel.deps.store.sessions().await? {
            kernel.ensure_actor(session)?;
        }
        *lock(&kernel.scheduler) = Some(scheduler.start(
            kernel.deps.store.clone(),
            kernel.deps.clock.clone(),
            kernel.sessions.clone(),
            kernel.deps.shutdown.clone(),
        ));
        Ok(kernel)
    }

    fn ensure_actor(&self, session: SessionRecord) -> Result<Arc<SessionHandle>, KernelError> {
        let mut sessions = lock(&self.sessions);
        if self.deps.shutdown.is_cancelled() {
            return Err(KernelError::ShuttingDown);
        }
        Ok(sessions
            .entry(session.id)
            .or_insert_with(|| SessionHandle::start(session, self.deps.clone()))
            .clone())
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

    /// Create a Session if needed and ensure its owner is running.
    pub async fn open_session(&self, name: &str) -> Result<SessionRecord, KernelError> {
        let session = self
            .deps
            .store
            .ensure_session(name, self.deps.clock.now())
            .await?;
        self.ensure_actor(session.clone())?;
        Ok(session)
    }

    /// Persist input before waking its owner; duplicate IDs remain harmless.
    pub async fn submit(
        &self,
        session: SessionId,
        event_id: EventId,
        text: String,
    ) -> Result<Accepted, KernelError> {
        let handle = self.handle(session)?;
        let accepted = self
            .deps
            .store
            .accept(&Event {
                id: event_id,
                session,
                source: EventSource::Cli,
                body: EventBody::UserMessage { text },
                received_at: self.deps.clock.now(),
            })
            .await?;
        handle.wake.notify_one();
        Ok(accepted)
    }

    /// Subscribe to newly committed facts; Log remains the durable source.
    pub fn subscribe(&self, session: SessionId) -> Result<broadcast::Receiver<Entry>, KernelError> {
        Ok(self.handle(session)?.entries.subscribe())
    }

    /// Signal the current activation; its actor settles work before ending.
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

    /// Inspect live state without involving the model.
    pub async fn status(&self) -> Result<Status, KernelError> {
        let node = self.deps.store.node().await?;
        let mut sessions: Vec<_> = lock(&self.sessions)
            .values()
            .map(|h| SessionStatus {
                session: h.session.clone(),
                running: lock(&h.run_cancel).is_some(),
                stopped: lock(&h.stopped).clone(),
            })
            .collect();
        sessions.sort_by(|a, b| a.session.name.cmp(&b.session.name));
        Ok(Status {
            node: node.id,
            safe_mode: node.safe_mode,
            provider: self.deps.snapshot.provider.code(),
            composer: self.deps.snapshot.composer.code(),
            sessions,
        })
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

    /// Commands to the single reminder owner, shared by CLI and model tools.
    pub fn schedules(&self) -> Schedules {
        self.schedules.clone()
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
                    handle.session.id
                )));
            }
        }
        let scheduler = lock(&self.scheduler).take();
        if let Some(task) = scheduler {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => failure = Some(error.into()),
                Err(error) => {
                    failure = Some(KernelError::TaskFailed(format!("Scheduler: {error}")))
                }
            }
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
