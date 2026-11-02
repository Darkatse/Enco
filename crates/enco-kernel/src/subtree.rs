use crate::{
    Clock, Halted, KernelError, SessionLookupError, Store, StoreError,
    session::{SessionDeps, SessionHandle, lock},
};
use enco_core::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub(crate) struct Subtrees {
    pub store: Arc<dyn Store>,
    pub clock: Arc<dyn Clock>,
    pub sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
    pub deps: OnceLock<Weak<SessionDeps>>,
}

impl Subtrees {
    pub async fn stop(
        &self,
        root: SessionId,
        source: EventSource,
        by: Option<SessionRef>,
        archive: bool,
    ) -> Result<Halted, KernelError> {
        let session = self
            .store
            .session(root)
            .await?
            .ok_or(KernelError::UnknownSession(root))?;
        let root = SessionRef {
            id: session.id,
            name: session.name,
        };
        let sessions = self
            .store
            .halt_subtree(
                root.id,
                source,
                Halt {
                    root: root.clone(),
                    by,
                },
                self.clock.now(),
                archive,
            )
            .await?;
        let mut handles = Vec::new();
        if sessions.is_empty() {
            if let Some(handle) = lock(&self.sessions).get(&root.id).cloned() {
                handles.push((root, handle));
            }
        } else {
            for session in &sessions {
                let handle = lock(&self.sessions).get(&session.id).cloned();
                let handle = match handle {
                    Some(handle) => Some(handle),
                    None => {
                        let record = self
                            .store
                            .session(session.id)
                            .await?
                            .ok_or(KernelError::UnknownSession(session.id))?;
                        self.deps
                            .get()
                            .and_then(Weak::upgrade)
                            .and_then(|deps| deps.ensure_actor(record))
                    }
                };
                if let Some(handle) = handle {
                    handles.push((session.clone(), handle));
                }
            }
        }
        for (_, handle) in &handles {
            if let Some(token) = lock(&handle.run_cancel).as_ref() {
                token.cancel();
            }
            handle.wake.notify_one();
        }
        let mut stopped = Vec::new();
        for (session, handle) in handles {
            if !self.wait(&handle).await? {
                stopped.push(session);
                continue;
            }
            if archive {
                let task = lock(&handle.task).take();
                if let Some(task) = task {
                    task.await
                        .map_err(|error| KernelError::TaskFailed(error.to_string()))?;
                }
            }
        }
        Ok(Halted { sessions, stopped })
    }

    /// Wait until no halt waits in this Session's Inbox; false when its actor stopped instead.
    async fn wait(&self, handle: &SessionHandle) -> Result<bool, KernelError> {
        loop {
            let changed = handle.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if lock(&handle.stopped).is_some() {
                return Ok(false);
            }
            if quiescent(self.store.as_ref(), handle.id).await? {
                return Ok(true);
            }
            changed.await;
        }
    }

    pub async fn child(
        &self,
        owner: SessionId,
        short: &str,
        archive: bool,
    ) -> Result<Halted, KernelError> {
        let parent = self
            .store
            .session(owner)
            .await?
            .ok_or(KernelError::UnknownSession(owner))?;
        let name = parent.name.child(short).map_err(SessionLookupError::from)?;
        let child = self
            .store
            .session_by_name(&name)
            .await?
            .ok_or_else(|| SessionLookupError::Unknown(name.to_string()))?;
        let by = SessionRef {
            id: parent.id,
            name: parent.name,
        };
        self.stop(child.id, EventSource::Parent, Some(by), archive)
            .await
    }
}

/// Whether no halt waits in this Session's Inbox.
///
/// An actor recovers before it serves and consumes a halt only between Runs, so once none waits,
/// every Run a halt stopped has ended.
pub(crate) async fn quiescent(store: &dyn Store, session: SessionId) -> Result<bool, StoreError> {
    Ok(!store
        .pending(session)
        .await?
        .iter()
        .any(|event| matches!(event.body, EventBody::Halt(..))))
}
