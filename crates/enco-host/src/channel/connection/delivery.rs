//! Log-derived deliveries and their durable settlements.
use super::*;
impl Connection {
    pub(super) async fn recover_delivery(&mut self) -> Result<(), ChannelError> {
        if let Some(delivery) = self.state.sending.clone() {
            self.settle(
                delivery,
                Settlement::Unknown {
                    failure: Failure {
                        code: code::INTERRUPTED.into(),
                        message: "process stopped during delivery; it will not be resent".into(),
                        retryable: false,
                    },
                },
                Vec::new(),
                Vec::new(),
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn next_delivery(&mut self) -> Result<(), ChannelError> {
        if self.in_flight.is_some() {
            return Ok(());
        }
        while let Some(session) = self.dirty.pop_first() {
            let mut cursor = self.state.outbound[&session].clone();
            let previous = cursor.processed;
            let entries = self.kernel.log(session, cursor.processed).await?;
            let outgoing = projection::project(&mut cursor, &self.identity, session, entries);
            if let Some((delivery, mut message)) = outgoing {
                let target = delivery.target.clone();
                if self.state.chats.get(&target) != Some(&session) {
                    let name = self.session_name(session).await?;
                    message.text = format!("[{name}]\n\n{}", message.text);
                }
                let file = match message.file {
                    Some(file) => match self.kernel.outgoing_file(&file).await {
                        Ok(file) => Some(file),
                        Err(KernelError::Store(error @ StoreError::Blob(_))) => {
                            self.settle(
                                delivery,
                                Settlement::Failed {
                                    failure: Failure {
                                        code: "channel.file_unavailable".into(),
                                        message: error.to_string(),
                                        retryable: false,
                                    },
                                },
                                Vec::new(),
                                Vec::new(),
                            )
                            .await?;
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    },
                    None => None,
                };
                let message = Outgoing {
                    text: message.text,
                    file,
                };
                let mut state = self.state.clone();
                state.sending = Some(delivery.clone());
                self.commit(state, None).await?;
                self.in_flight = Some(InFlight {
                    delivery,
                    task: self.send(target, message),
                });
                break;
            }
            if previous != cursor.processed {
                let mut state = self.state.clone();
                state.outbound.insert(session, cursor);
                self.commit(state, None).await?;
            }
        }
        Ok(())
    }

    fn send(
        &self,
        target: String,
        message: Outgoing<FileData>,
    ) -> JoinHandle<Result<transport::Sent, ChannelError>> {
        tokio::spawn(transport::send(
            self.adapter.clone(),
            self.kernel.clone(),
            self.generation,
            target,
            message,
            self.stop.clone(),
            self.pacing.clone(),
        ))
    }

    pub(super) async fn sent(
        &mut self,
        flight: InFlight,
        result: Result<Result<transport::Sent, ChannelError>, tokio::task::JoinError>,
    ) -> Result<Option<Fault>, ChannelError> {
        let sent = result.map_err(|error| ChannelError::Task(error.to_string()))??;
        self.settle(flight.delivery, sent.outcome, sent.messages, sent.origins)
            .await?;
        Ok(sent.fatal)
    }

    /// Record the delivery's outcome and the protocol's IDs for the parts confirmed sent.
    pub(super) async fn settle(
        &mut self,
        delivery: Delivery,
        outcome: Settlement,
        messages: Vec<String>,
        origins: Vec<MappingOrigin>,
    ) -> Result<(), ChannelError> {
        let mut state = self.state.clone();
        state.sending = None;
        state.outbound.insert(
            delivery.session,
            Cursor {
                processed: Some(delivery.pos),
                target: Some(delivery.target.clone()),
            },
        );
        self.dirty.insert(delivery.session);
        self.commit(
            state,
            Some(DeliverySettlement {
                delivery,
                outcome,
                messages,
                origins,
                at: self.clock.now(),
            }),
        )
        .await
    }
}
