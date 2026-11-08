mod admission;
mod commit;
mod tail;

use crate::{Clock, KernelError, Store, limits::SESSION_BROADCAST_CAPACITY, snapshot::Snapshot};
use enco_core::*;
use std::sync::{Arc, Mutex, MutexGuard};
use tail::LogTail;
use tokio::{
    sync::{Notify, broadcast},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct SessionDeps {
    pub home: std::path::PathBuf,
    pub store: Arc<dyn Store>,
    pub clock: Arc<dyn Clock>,
    pub registry: Arc<crate::Registry>,
    pub snapshot: Arc<Snapshot>,
    pub shutdown: CancellationToken,
    pub subtrees: Arc<crate::subtree::Subtrees>,
    pub slots: Arc<crate::child_slots::ChildSlots>,
}

pub(crate) struct SessionHandle {
    pub id: SessionId,
    pub wake: Notify,
    pub changed: Notify,
    pub entries: broadcast::Sender<Entry>,
    pub run_cancel: Mutex<Option<CancellationToken>>,
    pub stopped: Mutex<Option<String>>,
    pub task: Mutex<Option<JoinHandle<()>>>,
}

impl SessionHandle {
    pub fn start(session: SessionRecord, deps: Arc<SessionDeps>) -> Arc<Self> {
        let handle = Arc::new(Self {
            id: session.id,
            wake: Notify::new(),
            changed: Notify::new(),
            entries: broadcast::channel(SESSION_BROADCAST_CAPACITY).0,
            run_cancel: Mutex::new(None),
            stopped: Mutex::new(None),
            task: Mutex::new(None),
        });
        let task_handle = handle.clone();
        let task = tokio::spawn(async move {
            let mut actor = SessionActor {
                session,
                deps,
                handle: task_handle,
                tail: LogTail::default(),
                next: 1,
                delivery: DeliveryProjection::default(),
            };
            if let Err(error) = actor.serve().await {
                tracing::error!(session = %actor.session.id, %error, "Session stopped");
                *lock(&actor.handle.stopped) = Some(error.to_string());
            }
            *lock(&actor.handle.run_cancel) = None;
            actor.handle.changed.notify_waiters();
            if actor.session.archived_at.is_some() && lock(&actor.handle.stopped).is_none() {
                lock(&actor.deps.subtrees.sessions).remove(&actor.session.id);
            }
        });
        *lock(&handle.task) = Some(task);
        handle
    }
}

pub(crate) struct SessionActor {
    pub session: SessionRecord,
    pub deps: Arc<SessionDeps>,
    pub handle: Arc<SessionHandle>,
    pub tail: LogTail,
    pub next: u64,
    /// What the committed Log hands to its reply destination, folded up to its last entry.
    pub delivery: DeliveryProjection,
}

impl SessionActor {
    async fn serve(&mut self) -> Result<(), KernelError> {
        let entries = crate::reach::read_back(self.deps.store.as_ref(), self.session.id, |end| {
            crate::reach::earliest([tail::reach(end), DeliveryProjection::reach(end)])
        })
        .await?;
        for entry in &entries {
            self.delivery.step(entry);
        }
        self.next = entries
            .iter()
            .rev()
            .find(|e| e.pos.epoch == self.session.binding.epoch)
            .map_or(1, |e| e.pos.seq.0 + 1);
        self.tail = LogTail::new(entries);
        let recovery = crate::recovery::recover(self.tail.entries());
        if !recovery.is_empty() {
            self.commit(recovery, None).await?;
        }
        loop {
            if self.deps.shutdown.is_cancelled() {
                return Ok(());
            }
            self.session = self
                .deps
                .store
                .session(self.session.id)
                .await?
                .ok_or(crate::StoreError::UnknownSession(self.session.id))?;
            let pending = self.deps.store.pending(self.session.id).await?;
            if let Some(last) = pending
                .iter()
                .rposition(|event| event.body.supersedes_inputs())
            {
                let consumed = pending
                    .into_iter()
                    .take(last + 1)
                    .map(|event| EntryBody::EventConsumed { event })
                    .collect();
                self.commit(consumed, None).await?;
                if self.session.archived_at.is_some() {
                    return Ok(());
                }
                continue;
            }
            // Compaction requests are consumed between Runs on their own; the inputs before them
            // wait for the next Run.
            if pending
                .iter()
                .any(|event| matches!(event.body, EventBody::Compact))
            {
                let consumed = pending
                    .into_iter()
                    .filter(|event| matches!(event.body, EventBody::Compact))
                    .map(|event| EntryBody::EventConsumed { event })
                    .collect();
                self.commit(consumed, None).await?;
                continue;
            }
            if self.session.archived_at.is_some() {
                return Ok(());
            }
            if pending.is_empty() && !crate::transcript::backtracked_to_input(self.tail.entries()) {
                tokio::select! {
                    _ = self.handle.wake.notified() => {},
                    _ = self.deps.shutdown.cancelled() => return Ok(()),
                }
                continue;
            }
            let permit = if self.session.parent.is_some() {
                let Some(permit) = self.child_permit().await? else {
                    continue;
                };
                Some(permit)
            } else {
                None
            };
            let token = self.deps.shutdown.child_token();
            *lock(&self.handle.run_cancel) = Some(token.clone());
            let result = self.run(token).await;
            *lock(&self.handle.run_cancel) = None;
            self.handle.changed.notify_waiters();
            drop(permit);
            result?;
        }
    }
}
