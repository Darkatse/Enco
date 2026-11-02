use super::*;

impl SessionActor {
    pub(super) async fn settle(
        &mut self,
        call: CallId,
        execution: Execution,
    ) -> Result<(), StoreError> {
        let mut delegated = None;
        let mut deliveries = Vec::new();
        let mut outcome = match execution {
            Execution::Result(outcome) => outcome,
            Execution::Delegation(request) => {
                let (fact, delivery) = self.delegation(request).await?;
                let short = fact.session.name.short_name();
                let value = if fact.created {
                    format!("Created child Session `{short}` and sent it the message.")
                } else {
                    format!("Sent the message to `{short}`.")
                }
                .into();
                delegated = Some(fact);
                deliveries.push(delivery);
                Outcome::Ok {
                    value,
                    images: Vec::new(),
                    outgoing: None,
                }
            }
        };
        let mut stored = Vec::new();
        if let Outcome::Ok { images, .. } = &outcome {
            for bytes in images {
                match crate::image::store(self.deps.store.as_ref(), bytes).await {
                    Ok(image) => stored.push(image),
                    Err(crate::ImageError::Store(error)) => return Err(error),
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
                "\n[image: {}, {} bytes; file: {}]",
                image.format.mime(),
                image.size,
                self.deps.store.blob_path(&image.content).display()
            ));
        }
        let body = EntryBody::ToolCallSettled {
            call,
            outcome: Settlement::from(&outcome),
            content,
            full,
            images: stored,
            outgoing,
            delegated,
        };
        match self.commit_checked(vec![body], None, deliveries).await? {
            CommitOutcome::Committed { .. } => Ok(()),
            CommitOutcome::Rejected(reason) => self.reject_settlement(call, reason).await,
        }
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
            }],
            None,
        )
        .await
    }
}
