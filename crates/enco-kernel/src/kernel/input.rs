//! Input acceptance and backtracking share one commit before waking or cancelling Session work.
use super::*;

impl Kernel {
    /// Persist input before waking its owner; duplicate IDs remain harmless.
    pub async fn submit(
        &self,
        session: SessionId,
        event_id: EventId,
        text: String,
        images: Vec<Image>,
    ) -> Result<Accepted, KernelError> {
        let accepted = self
            .accept(
                &[Event {
                    id: event_id,
                    session,
                    source: EventSource::Cli,
                    body: EventBody::UserMessage {
                        text,
                        images,
                        unviewable: vec![],
                        earlier: vec![],
                        quote: None,
                    },
                    received_at: self.deps.clock.now(),
                }],
                None,
            )
            .await?;
        Ok(accepted[0])
    }

    /// Take the conversation back to a recorded position, as the owner asked from the CLI.
    ///
    /// Like `submit`, a repeated event ID is accepted once.
    pub async fn backtrack(
        &self,
        session: SessionId,
        event_id: EventId,
        to: LogPos,
    ) -> Result<Accepted, KernelError> {
        let accepted = self
            .accept(
                &[Event {
                    id: event_id,
                    session,
                    source: EventSource::Cli,
                    body: EventBody::Backtrack { to },
                    received_at: self.deps.clock.now(),
                }],
                None,
            )
            .await?;
        Ok(accepted[0])
    }

    /// Accept inputs and connection progress before waking the affected Session owners.
    ///
    /// A Session the connection creates with this acceptance starts once it is committed. A
    /// backtrack must point at a recorded position other than another backtrack; a newly accepted
    /// one signals the Session's current Run, which stops at its next boundary.
    pub async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Vec<Accepted>, KernelError> {
        if self.deps.shutdown.is_cancelled() {
            return Err(KernelError::ShuttingDown);
        }
        let created = connection.and_then(|write| write.session.as_ref());
        let mut handles = HashMap::new();
        for event in events {
            if created.is_none_or(|session| session.id != event.session) {
                handles.insert(event.session, self.handle(event.session)?);
            }
            if let EventBody::Backtrack { to } = event.body {
                self.check_backtrack(event.session, to).await?;
            }
        }
        let accepted = match created {
            Some(session) => {
                self.deps
                    .registry
                    .accept_creating(session, events, connection)
                    .await?
            }
            None => self.deps.store.accept(events, connection).await?,
        };
        if let Some(session) = accepted.session {
            handles.insert(session.id, self.ensure_actor(session)?);
        }
        for (event, accepted) in events.iter().zip(&accepted.events) {
            handles[&event.session].wake.notify_one();
            // A resent backtrack already took effect; the Run now going may be a later one.
            if *accepted == Accepted::New && crate::transcript::is_backtrack(event) {
                self.cancel(event.session)?;
            }
        }
        Ok(accepted.events)
    }

    async fn check_backtrack(&self, session: SessionId, to: LogPos) -> Result<(), KernelError> {
        let rejected = |reason| KernelError::Backtrack {
            session,
            to,
            reason,
        };
        let entries = self.deps.store.log(session, None).await?;
        let entry = entries
            .iter()
            .find(|entry| entry.pos == to)
            .ok_or_else(|| rejected("the position is not in its Log"))?;
        match &entry.body {
            EntryBody::EventConsumed { event } if crate::transcript::is_backtrack(event) => {
                Err(rejected("the position is another backtrack"))
            }
            _ => Ok(()),
        }
    }
}
