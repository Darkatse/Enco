use crate::{
    Clock, Commit, KernelConfig, KernelError, Store, StoreError,
    limits::SESSION_BROADCAST_CAPACITY, snapshot::Snapshot,
};
use enco_core::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::{
    sync::{Notify, broadcast},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct SessionDeps {
    pub store: Arc<dyn Store>,
    pub clock: Arc<dyn Clock>,
    pub registry: Arc<crate::Registry>,
    pub profiles: BTreeMap<String, crate::Profile>,
    pub snapshot: Arc<Snapshot>,
    pub config: KernelConfig,
    pub shutdown: CancellationToken,
}

pub(crate) struct SessionHandle {
    pub id: SessionId,
    pub wake: Notify,
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
                entries: vec![],
                next: 1,
            };
            if let Err(error) = actor.serve().await {
                tracing::error!(session = %actor.session.id, %error, "Session stopped");
                *lock(&actor.handle.stopped) = Some(error.to_string());
            }
            *lock(&actor.handle.run_cancel) = None;
        });
        *lock(&handle.task) = Some(task);
        handle
    }
}

pub(crate) struct SessionActor {
    pub session: SessionRecord,
    pub deps: Arc<SessionDeps>,
    pub handle: Arc<SessionHandle>,
    // Keep the full Log in memory; project incrementally if long Sessions make scans costly.
    pub entries: Vec<Entry>,
    pub next: u64,
}

impl SessionActor {
    async fn serve(&mut self) -> Result<(), KernelError> {
        self.entries = self.deps.store.log(self.session.id, None).await?;
        self.next = self
            .entries
            .iter()
            .rev()
            .find(|e| e.pos.epoch == self.session.binding.epoch)
            .map_or(1, |e| e.pos.seq.0 + 1);
        let recovery = crate::recovery::recover(&self.entries);
        if !recovery.is_empty() {
            self.commit(recovery, vec![]).await?;
        }
        loop {
            if self.deps.shutdown.is_cancelled() {
                return Ok(());
            }
            if self.deps.store.pending(self.session.id).await?.is_empty() {
                tokio::select! {
                    _ = self.handle.wake.notified() => {},
                    _ = self.deps.shutdown.cancelled() => return Ok(()),
                }
            } else {
                let token = self.deps.shutdown.child_token();
                *lock(&self.handle.run_cancel) = Some(token.clone());
                let result = self.run(token).await;
                *lock(&self.handle.run_cancel) = None;
                result?;
            }
        }
    }

    pub(crate) async fn commit(
        &mut self,
        bodies: Vec<EntryBody>,
        consumed: Vec<EventId>,
    ) -> Result<(), StoreError> {
        let entries: Vec<_> = bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| Entry {
                pos: LogPos {
                    epoch: self.session.binding.epoch,
                    seq: Seq(self.next + i as u64),
                },
                at: self.deps.clock.now().to_utc(),
                body,
            })
            .collect();
        self.deps
            .store
            .commit(
                self.session.id,
                Commit {
                    entries: entries.clone(),
                    consumed,
                },
            )
            .await?;
        self.next += entries.len() as u64;
        for entry in entries {
            self.entries.push(entry.clone());
            let _ = self.handle.entries.send(entry);
        }
        Ok(())
    }
}
