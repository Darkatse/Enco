//! Input acceptance and backtracking share one commit before waking or cancelling Session work.
use super::*;

impl Kernel {
    /// Persist input before waking its owner; duplicate IDs remain harmless.
    pub async fn submit(
        &self,
        session: SessionId,
        event_id: EventId,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<Accepted, KernelError> {
        let accepted = self
            .accept(
                &[Event {
                    id: event_id,
                    session,
                    source: EventSource::Cli,
                    body: EventBody::UserMessage {
                        text,
                        attachments,
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

    /// Take the conversation back to a recorded position, or its start when `to` is absent.
    ///
    /// Like `submit`, a repeated event ID is accepted once.
    pub async fn backtrack(
        &self,
        session: SessionId,
        event_id: EventId,
        to: Option<LogPos>,
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

    /// Point the Session at `path` on this node from its next Round, without checking it.
    ///
    /// It is a command into its Inbox, consumed at the next Round boundary, at once while the
    /// Session is idle, and once it runs again if it has stopped.
    pub async fn set_working_dir(
        &self,
        session: SessionId,
        path: std::path::PathBuf,
    ) -> Result<WorkingDir, KernelError> {
        let working_dir = WorkingDir::new(self.deps.store.node().await?.id, path)?;
        self.apply(
            session,
            EventBody::SetWorkingDir {
                working_dir: working_dir.clone(),
            },
        )
        .await?;
        Ok(working_dir)
    }

    /// Select a profile from the Session's next Round: a command into its Inbox, accepted only
    /// while the configuration defines the profile.
    pub async fn set_profile(
        &self,
        session: SessionId,
        profile: &ProfileName,
    ) -> Result<(), KernelError> {
        self.apply(
            session,
            EventBody::SetProfile {
                profile: profile.clone(),
            },
        )
        .await
    }

    /// The active schedules of every unarchived Session, or of one, in due order.
    ///
    /// They are the effective view: an accepted cancel or reminder counts before its Session
    /// consumes it.
    pub async fn schedules(
        &self,
        session: Option<SessionId>,
    ) -> Result<Vec<Scheduled>, KernelError> {
        let timezone = self.deps.registry.view().config.kernel.timezone;
        let schedules = self.deps.store.schedules(session).await?;
        Ok(crate::scheduler::in_due_order(schedules, timezone)?)
    }

    /// Stop an active schedule: a command into the Inbox of the Session it belongs to.
    ///
    /// No reminder is written once the command is accepted; the Session records the cancel at
    /// its next Round boundary, at once while idle, and once it runs again if it has stopped.
    /// Returns false when no unarchived Session has the schedule active when the command would be
    /// written.
    pub async fn cancel_schedule(&self, schedule: ScheduleId) -> Result<bool, KernelError> {
        let Some(owner) = self
            .deps
            .store
            .schedules(None)
            .await?
            .into_iter()
            .find(|row| row.id == schedule && row.state == ScheduleState::Active)
            .map(|row| row.session)
        else {
            return Ok(false);
        };
        let cancelled = match self
            .command(owner, EventBody::CancelSchedule { schedule })
            .await?
        {
            Accepted::New | Accepted::Duplicate => true,
            // Cancelled, reminded for the last time, or archived since it was read.
            Accepted::Inactive | Accepted::Archived => false,
        };
        self.deps.scheduler.changed.notify_one();
        Ok(cancelled)
    }

    /// Accept an owner's command acting on the Session, as the Inbox's write function answers.
    async fn command(&self, session: SessionId, body: EventBody) -> Result<Accepted, KernelError> {
        let event = Event {
            id: EventId::new(),
            session,
            source: EventSource::Cli,
            body,
            received_at: self.deps.clock.now(),
        };
        Ok(self.accept(std::slice::from_ref(&event), None).await?[0])
    }

    /// Accept an owner's command that names no schedule; an archived Session takes none.
    async fn apply(&self, session: SessionId, body: EventBody) -> Result<(), KernelError> {
        match self.command(session, body).await? {
            Accepted::New | Accepted::Duplicate => Ok(()),
            Accepted::Inactive => Err(StoreError::Inbox(format!(
                "session {session} refused a command naming no schedule as naming an inactive one"
            ))
            .into()),
            Accepted::Archived => {
                let record = self
                    .deps
                    .store
                    .session(session)
                    .await?
                    .ok_or(KernelError::UnknownSession(session))?;
                Err(SessionLookupError::Archived {
                    name: record.name.into(),
                    ids: vec![session],
                }
                .into())
            }
        }
    }

    /// Inspect live state without involving the model.
    pub async fn status(&self) -> Result<Status, KernelError> {
        let view = self.deps.registry.view();
        let node = self.deps.store.node().await?;
        let records = self.deps.store.sessions().await?;
        let handles = lock(&self.sessions);
        let sessions = records
            .into_iter()
            .map(|session| self.session_status(&handles, session))
            .collect();
        Ok(Status {
            timezone: view.config.kernel.timezone,
            node: node.id,
            safe_mode: node.safe_mode,
            profiles: view.config.kernel.profiles.keys().cloned().collect(),
            sessions,
            scheduler_stopped: lock(&self.deps.scheduler.stopped).clone(),
        })
    }

    /// Accept inputs and connection progress before waking the affected Session owners.
    ///
    /// A Session the connection creates with this acceptance starts once it is committed. An
    /// input to an archived Session is not written and its result says so; the connection
    /// progress commits all the same. A Session whose actor stopped takes only the commands
    /// acting on itself, and fails anything else with `SessionStopped`. A backtrack must point at
    /// a recorded position other than a command; a newly accepted backtrack or halt signals the
    /// Session's current Run, which stops at its next boundary.
    pub async fn accept(
        &self,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Vec<Accepted>, KernelError> {
        if self.deps.shutdown.is_cancelled() {
            return Err(KernelError::ShuttingDown);
        }
        for event in events {
            // Whether the Session exists or is archived is the transaction's to say. A stopped
            // Session takes the commands acting on itself, which hold in its effective view from
            // acceptance and which it records once it runs again; inputs and commands on the
            // conversation wait for it to act, so they are refused.
            if event.body.command_on() != Some(CommandOn::Session)
                && let Some(handle) = lock(&self.sessions).get(&event.session)
                && let Some(reason) = lock(&handle.stopped).clone()
            {
                return Err(KernelError::SessionStopped(event.session, reason));
            }
            if let EventBody::Backtrack { to: Some(to) } = event.body {
                self.check_backtrack(event.session, to).await?;
            }
        }
        // A created Session or a profile command names a profile; it is accepted under the
        // registry's committer, which also removes profiles.
        let profiles: Vec<&ProfileName> = connection
            .and_then(|write| write.session.as_ref())
            .map(|session| &session.profile)
            .into_iter()
            .chain(events.iter().filter_map(|event| match &event.body {
                EventBody::SetProfile { profile } => Some(profile),
                _ => None,
            }))
            .collect();
        let accepted = if profiles.is_empty() {
            self.deps
                .store
                .accept(events, connection)
                .await
                .map_err(refused)?
        } else {
            self.deps
                .registry
                .accept_naming(&profiles, events, connection)
                .await
                .map_err(|error| match error {
                    RegistryError::UnknownProfile(name) => KernelError::UnknownProfile(name),
                    RegistryError::Store(error) => refused(error),
                    error => error.into(),
                })?
        };
        if let Some(session) = accepted.session {
            self.ensure_actor(session)?;
        }
        for (event, accepted) in events.iter().zip(&accepted.events) {
            // Nothing was written for it.
            if matches!(accepted, Accepted::Archived | Accepted::Inactive) {
                continue;
            }
            let handle = lock(&self.sessions)
                .get(&event.session)
                .cloned()
                .ok_or(KernelError::UnknownSession(event.session))?;
            handle.wake.notify_one();
            // A resent command already took effect; the Run now going may be a later one.
            if *accepted == Accepted::New
                && event.body.supersedes_inputs()
                && let Some(token) = lock(&handle.run_cancel).as_ref()
            {
                token.cancel();
            }
        }
        Ok(accepted.events)
    }

    /// Whether `session` can accept a compaction request now: it is running, or its current
    /// conversation has a complete Round to compact.
    pub async fn can_request_compaction(&self, session: SessionId) -> Result<bool, KernelError> {
        if super::running(&lock(&self.sessions), session) {
            return Ok(true);
        }
        let entries = crate::reach::read_back(
            self.deps.store.as_ref(),
            session,
            crate::transcript::transcript_reach,
        )
        .await?;
        let conversation = crate::transcript::conversation(&entries);
        Ok(crate::transcript::has_compactable_round(&conversation))
    }

    async fn check_backtrack(&self, session: SessionId, to: LogPos) -> Result<(), KernelError> {
        let rejected = |reason| KernelError::Backtrack {
            session,
            to,
            reason,
        };
        let entry = self
            .deps
            .store
            .log_entry(session, to)
            .await?
            .ok_or_else(|| rejected("the position is not in its Log"))?;
        match &entry.body {
            EntryBody::EventConsumed { event } if event.body.is_command() => {
                Err(rejected("the position is a command"))
            }
            _ => Ok(()),
        }
    }
}

/// A failed acceptance as the kernel reports it: whether the Session exists is the transaction's
/// to say, and an Event to one that does not names it.
fn refused(error: StoreError) -> KernelError {
    match error {
        StoreError::UnknownSession(session) => KernelError::UnknownSession(session),
        error => error.into(),
    }
}
