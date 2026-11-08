use super::*;
use crate::{Commit, CommitOutcome, PlanWrite, SessionDelivery, SessionDeliveryKind, StoreError};

impl SessionActor {
    pub(crate) async fn commit(
        &mut self,
        bodies: Vec<EntryBody>,
        plan: Option<Arc<PlanWrite>>,
    ) -> Result<(), StoreError> {
        match self.commit_checked(bodies, plan, Vec::new()).await? {
            CommitOutcome::Committed { .. } => Ok(()),
            CommitOutcome::Rejected(reason) => Err(StoreError::Backend(format!(
                "non-tool commit rejected: {reason}"
            ))),
        }
    }

    pub(crate) async fn commit_checked(
        &mut self,
        bodies: Vec<EntryBody>,
        plan: Option<Arc<PlanWrite>>,
        mut deliveries: Vec<SessionDelivery>,
    ) -> Result<CommitOutcome, StoreError> {
        let entries: Vec<_> = bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| Entry {
                pos: LogPos {
                    epoch: self.session.binding.epoch,
                    seq: Seq(self.next + i as u64),
                },
                at: self.deps.clock.now(),
                body,
            })
            .collect();
        let mut delivery = self.delivery.clone();
        for entry in &entries {
            if let Some(boundary) = delivery.step(entry)
                && let Some(parent) = &self.session.parent
                && boundary.destination == Some(ReplyDestination::Parent)
                && let Some(content) = boundary.deliverable
            {
                deliveries.push(self.report(parent.id, entry, content));
            }
        }
        let write = Commit {
            entries: entries.clone(),
            plan,
            deliveries,
        };
        let outcome = self
            .deps
            .registry
            .commit_session(self.session.id, write)
            .await?;
        if let CommitOutcome::Committed { delivered } = &outcome {
            self.delivery = delivery;
            self.next += entries.len() as u64;
            for entry in &entries {
                let _ = self.handle.entries.send(entry.clone());
            }
            self.tail
                .extend(self.deps.store.as_ref(), self.session.id, entries)
                .await?;
            self.handle.changed.notify_waiters();
            for id in delivered {
                let session = self
                    .deps
                    .store
                    .session(*id)
                    .await?
                    .ok_or(StoreError::UnknownSession(*id))?;
                if let Some(handle) = self.deps.ensure_actor(session) {
                    handle.wake.notify_one();
                }
            }
        }
        Ok(outcome)
    }

    /// Record a boundary's content as an input from this child to its parent.
    fn report(&self, parent: SessionId, entry: &Entry, content: Deliverable) -> SessionDelivery {
        let (body, kind) = match content {
            Deliverable::Reply(text) => (
                EventBody::SessionMessage(Outgoing { text, file: None }),
                SessionDeliveryKind::Boundary,
            ),
            Deliverable::Message(outgoing) => (
                EventBody::SessionMessage(outgoing),
                SessionDeliveryKind::Message,
            ),
            Deliverable::RunEnded(end) => (
                EventBody::ChildRunEnded { end },
                SessionDeliveryKind::Boundary,
            ),
        };
        SessionDelivery {
            event: Event {
                id: EventId::new(),
                session: parent,
                source: EventSource::Child(SessionRef {
                    id: self.session.id,
                    name: self.session.name.clone(),
                }),
                body,
                received_at: entry.at,
            },
            kind,
        }
    }
}

impl SessionDeps {
    pub fn ensure_actor(self: &Arc<Self>, session: SessionRecord) -> Option<Arc<SessionHandle>> {
        let mut sessions = lock(&self.subtrees.sessions);
        if self.shutdown.is_cancelled() {
            return None;
        }
        Some(
            sessions
                .entry(session.id)
                .or_insert_with(|| SessionHandle::start(session, self.clone()))
                .clone(),
        )
    }
}
