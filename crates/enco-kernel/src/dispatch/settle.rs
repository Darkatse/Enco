use super::*;

impl SessionActor {
    pub(super) async fn settle(
        &mut self,
        call: CallId,
        execution: Execution,
    ) -> Result<(), StoreError> {
        let Executed {
            mut outcome,
            delegated,
            schedule,
            deliveries,
        } = self.executed(execution).await?;
        let mut stored = Vec::new();
        if let Outcome::Ok { images, .. } = &outcome {
            for bytes in images {
                match crate::image::store(self.deps.store.as_ref(), bytes).await? {
                    Ok(image) => stored.push(image),
                    Err(error) => {
                        outcome = Outcome::Failed {
                            failure: Failure {
                                code: code::TOOL_FAILED.into(),
                                message: error.to_string(),
                                retryable: false,
                            },
                        };
                        stored.clear();
                        break;
                    }
                }
            }
        }
        let outgoing = if let Outcome::Ok { outgoing, .. } = &mut outcome
            && let Some(outgoing) = outgoing.take()
        {
            Some(crate::outgoing::store(self.deps.store.as_ref(), outgoing).await?)
        } else {
            None
        };
        let mut content = render_text(&outcome);
        let mut full = None;
        if content.len() > limits::TOOL_RESULT_INLINE_BYTES {
            let hash = self.deps.store.put_blob(content.as_bytes()).await?;
            let size = content.len();
            content.truncate(content.floor_char_boundary(limits::TOOL_RESULT_PREVIEW_BYTES));
            let path = self.deps.store.blob_path(&hash);
            content.push_str(&format!(
                "\n[truncated: {size} bytes; full result: {}; it may be cleaned up later]",
                path.display(),
            ));
            full = Some(hash);
        }
        for image in &stored {
            content.push_str(&format!(
                "\n[image: {}, {} bytes, at {}]",
                image.format.mime(),
                image.size,
                self.deps.store.blob_path(&image.content).display()
            ));
        }
        // The Scheduler reads the schedules again once a change to them is committed.
        let changes_schedules = schedule.is_some();
        let body = EntryBody::ToolCallSettled {
            call,
            outcome: Settlement::from(&outcome),
            content,
            full,
            images: stored,
            outgoing,
            delegated,
            schedule,
        };
        match self.commit_checked(vec![body], None, deliveries).await? {
            CommitOutcome::Committed { .. } => {
                if changes_schedules {
                    self.deps.scheduler.changed.notify_one();
                }
                Ok(())
            }
            CommitOutcome::Rejected(reason) => self.reject_settlement(call, reason).await,
        }
    }

    /// The outcome an execution settles with, and the facts it records alongside.
    async fn executed(&self, execution: Execution) -> Result<Executed, StoreError> {
        let ok = |value| Outcome::Ok {
            value,
            images: Vec::new(),
            outgoing: None,
        };
        Ok(match execution {
            Execution::Result(outcome) => Executed::outcome(outcome),
            Execution::Schedule(request) => match self.schedule_call(request).await? {
                Ok((change, value)) => Executed {
                    schedule: change,
                    ..Executed::outcome(ok(value))
                },
                Err(failure) => Executed::outcome(Outcome::Failed { failure }),
            },
            Execution::Delegation(request) => match self.delegation(request).await? {
                Ok((fact, delivery)) => {
                    let value = match &fact.target {
                        DelegationTarget::Created(creation) => format!(
                            "Created child Session `{}` and sent it the message.",
                            creation.name.short_name()
                        ),
                        DelegationTarget::Existing(child) => {
                            format!("Sent the message to `{}`.", child.name.short_name())
                        }
                    };
                    Executed {
                        delegated: Some(fact),
                        deliveries: vec![delivery],
                        ..Executed::outcome(ok(value.into()))
                    }
                }
                Err(failure) => Executed::outcome(Outcome::Failed { failure }),
            },
        })
    }

    async fn reject_settlement(
        &mut self,
        call: CallId,
        reason: CommitRejection,
    ) -> Result<(), StoreError> {
        let failure = Failure {
            code: code::TOOL_FAILED.into(),
            message: reason.to_string(),
            retryable: false,
        };
        let outcome = Outcome::Failed { failure };
        self.commit(
            vec![EntryBody::ToolCallSettled {
                call,
                outcome: Settlement::from(&outcome),
                content: render_text(&outcome),
                full: None,
                images: Vec::new(),
                outgoing: None,
                delegated: None,
                schedule: None,
            }],
            None,
        )
        .await
    }
}

/// An execution's outcome, with what its settlement records and delivers.
struct Executed {
    outcome: Outcome,
    delegated: Option<Delegated>,
    schedule: Option<ScheduleChange>,
    deliveries: Vec<crate::SessionDelivery>,
}

impl Executed {
    fn outcome(outcome: Outcome) -> Self {
        Self {
            outcome,
            delegated: None,
            schedule: None,
            deliveries: Vec::new(),
        }
    }
}
