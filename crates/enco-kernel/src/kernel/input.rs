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
    /// A Session the connection creates with this acceptance starts once it is committed. An
    /// input to an archived Session is not written and its result says so; the connection
    /// progress commits all the same. A backtrack must point at a recorded position other than a
    /// command; a newly accepted command signals the Session's current Run, which stops at its
    /// next boundary.
    pub async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Vec<Accepted>, KernelError> {
        if self.deps.shutdown.is_cancelled() {
            return Err(KernelError::ShuttingDown);
        }
        for event in events {
            // Whether the Session exists or is archived is the transaction's to say.
            if let Some(handle) = lock(&self.sessions).get(&event.session)
                && let Some(reason) = lock(&handle.stopped).clone()
            {
                return Err(KernelError::SessionStopped(event.session, reason));
            }
            if let EventBody::Backtrack { to } = event.body {
                self.check_backtrack(event.session, to).await?;
            }
        }
        let accepted = match connection.and_then(|write| write.session.as_ref()) {
            Some(session) => {
                self.deps
                    .registry
                    .accept_creating(session, events, connection)
                    .await?
            }
            None => self.deps.store.accept(events, connection).await?,
        };
        if let Some(session) = accepted.session {
            self.ensure_actor(session)?;
        }
        for (event, accepted) in events.iter().zip(&accepted.events) {
            if *accepted == Accepted::Archived {
                continue;
            }
            let handle = lock(&self.sessions)
                .get(&event.session)
                .cloned()
                .ok_or(KernelError::UnknownSession(event.session))?;
            handle.wake.notify_one();
            // A resent command already took effect; the Run now going may be a later one.
            if *accepted == Accepted::New
                && crate::transcript::is_command(event)
                && let Some(token) = lock(&handle.run_cancel).as_ref()
            {
                token.cancel();
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
            EntryBody::EventConsumed {
                event:
                    Event {
                        body: EventBody::Halt(..),
                        ..
                    },
            } => Err(rejected("the position is a halt")),
            EntryBody::EventConsumed {
                event:
                    Event {
                        body: EventBody::Backtrack { .. },
                        ..
                    },
            } => Err(rejected("the position is another backtrack")),
            _ => Ok(()),
        }
    }
}
