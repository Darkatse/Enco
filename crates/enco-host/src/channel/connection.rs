use super::{projection, transport, *};
use enco_kernel::ConnectionWrite;
use std::collections::{BTreeSet, VecDeque};
use tokio::{
    sync::{broadcast, mpsc},
    task::{JoinHandle, JoinSet},
};

struct Connection {
    kernel: Arc<Kernel>,
    adapter: Arc<dyn Adapter>,
    identity: Identity,
    owner_id: String,
    state: State,
    clock: Arc<dyn Clock>,
    stop: CancellationToken,
    readers: JoinSet<Result<(), ChannelError>>,
    watched: BTreeSet<SessionId>,
    dirty: BTreeSet<SessionId>,
    wake: mpsc::Sender<SessionId>,
    // Immediate command receipts are interface responses, not Log-derived deliveries.
    receipts: VecDeque<Receipt>,
    in_flight: Option<InFlight>,
}

struct Receipt {
    target: String,
    text: String,
}

/// Runtime sending kind is explicit; persisted `State::sending` is only the recovery checkpoint.
enum InFlight {
    Receipt(JoinHandle<transport::Sent>),
    Delivery(Delivery, JoinHandle<transport::Sent>),
}

impl InFlight {
    fn task(&mut self) -> &mut JoinHandle<transport::Sent> {
        match self {
            Self::Receipt(task) | Self::Delivery(_, task) => task,
        }
    }
}

/// Operational failure is observable state; only cleanup failures are returned by shutdown.
pub(super) struct Exit {
    pub stopped: Option<ChannelError>,
    pub shutdown: Result<(), ChannelError>,
}

pub(super) async fn run(
    kernel: Arc<Kernel>,
    adapter: Arc<dyn Adapter>,
    owner_id: String,
    clock: Arc<dyn Clock>,
    stop: CancellationToken,
) -> Exit {
    let state = match restore(&kernel, adapter.as_ref()).await {
        Ok(state) => state,
        Err(error) => {
            return Exit {
                stopped: Some(error),
                shutdown: Ok(()),
            };
        }
    };
    let (wake, wakes) = mpsc::channel(crate::limits::CHANNEL_WAKE_CAPACITY);
    let mut connection = Connection {
        identity: adapter.identity(),
        kernel,
        adapter,
        owner_id,
        state,
        clock,
        stop: stop.child_token(),
        readers: JoinSet::new(),
        watched: BTreeSet::new(),
        dirty: BTreeSet::new(),
        wake,
        receipts: VecDeque::new(),
        in_flight: None,
    };
    // Run cleanup even if recovery, subscription or acceptance fails.
    let result = connection.serve(wakes).await;
    connection.quiesce(result).await
}

/// The committed state, or a fresh one for a connection seen for the first time.
async fn restore(kernel: &Kernel, adapter: &dyn Adapter) -> Result<State, ChannelError> {
    Ok(match kernel.connection(&adapter.identity().key()).await? {
        Some(state) => serde_json::from_value(state)?,
        None => State {
            protocol: adapter.initial_state(),
            chats: BTreeMap::new(),
            outbound: BTreeMap::new(),
            sending: None,
        },
    })
}

impl Connection {
    /// Settle a delivery the last process left in flight and follow the Sessions this
    /// connection delivers for, then handle input, sends and wakes in order until stopped.
    async fn serve(&mut self, mut wakes: mpsc::Receiver<SessionId>) -> Result<(), ChannelError> {
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
            )
            .await?;
        }
        let sessions: Vec<_> = self.state.outbound.keys().copied().collect();
        for session in sessions {
            self.follow(session)?;
        }
        let (batches, mut incoming) = mpsc::channel(1);
        self.readers.spawn(transport::poll(
            self.adapter.clone(),
            self.kernel.clone(),
            self.owner_id.clone(),
            self.state.protocol.clone(),
            batches,
            self.stop.clone(),
        ));
        loop {
            if self.stop.is_cancelled() {
                return Ok(());
            }
            self.next_delivery().await?;
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Ok(()),
                result = async {
                    match &mut self.in_flight {
                        Some(flight) => flight.task().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let flight = self.in_flight.take().ok_or_else(|| ChannelError::Task("send completed without an in-flight operation".into()))?;
                    if let Some(error) = self.sent(flight, result).await? { return Err(error.into()); }
                },
                Some(result) = self.readers.join_next() => {
                    result.map_err(|error| ChannelError::Task(error.to_string()))??;
                    return Err(ChannelError::Task("channel reader stopped unexpectedly".into()));
                }
                Some(batch) = incoming.recv() => {
                    for update in batch.updates { self.accept(update).await?; }
                    let _ = batch.committed.send(self.state.protocol.clone());
                }
                Some(session) = wakes.recv() => { self.dirty.insert(session); }
            }
        }
    }

    /// Signal stop, then wait for the send in flight and every reader to finish.
    async fn quiesce(mut self, result: Result<(), ChannelError>) -> Exit {
        self.stop.cancel();
        let mut exit = Exit {
            stopped: result.err(),
            shutdown: Ok(()),
        };
        if let Some(mut flight) = self.in_flight.take() {
            let result = flight.task().await;
            match self.sent(flight, result).await {
                Ok(Some(error)) => {
                    exit.stopped.get_or_insert(error.into());
                }
                Ok(None) => {}
                Err(error) => exit.shutdown = Err(error),
            }
        }
        while let Some(result) = self.readers.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    exit.stopped.get_or_insert(error);
                }
                Err(error) => exit.shutdown = Err(ChannelError::Task(error.to_string())),
            }
        }
        exit
    }

    async fn commit(
        &mut self,
        state: State,
        events: &[Event],
        settlement: Option<DeliverySettlement>,
    ) -> Result<(), ChannelError> {
        self.kernel
            .accept(
                events,
                Some(&ConnectionWrite {
                    key: self.identity.key(),
                    state: serde_json::to_value(&state)?,
                    settlement,
                }),
            )
            .await?;
        self.state = state;
        Ok(())
    }

    fn follow(&mut self, session: SessionId) -> Result<(), ChannelError> {
        if self.watched.contains(&session) {
            return Ok(());
        }
        // Subscribe before marking dirty; every commit is covered by Log or a later wake.
        let mut entries = self.kernel.subscribe(session)?;
        self.watched.insert(session);
        self.dirty.insert(session);
        let wake = self.wake.clone();
        let stop = self.stop.clone();
        self.readers.spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    event = entries.recv() => event,
                };
                match event {
                    Ok(Entry { body: EntryBody::RoundEnded { .. } | EntryBody::RunEnded { .. }, .. })
                    | Err(broadcast::error::RecvError::Lagged(_)) => {
                        tokio::select! {
                            _ = stop.cancelled() => return Ok(()),
                            result = wake.send(session) => result.map_err(|_| ChannelError::Task("connection stopped".into()))?,
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return Err(ChannelError::Task(format!("Session {session} subscription closed"))),
                    _ => {},
                }
            }
        });
        Ok(())
    }

    async fn accept(&mut self, update: transport::Prepared) -> Result<(), ChannelError> {
        let mut state = self.state.clone();
        state.protocol = update.protocol;
        let Some(transport::OwnerInput {
            conversation: target,
            action,
        }) = update.owner
        else {
            return self.commit(state, &[], None).await;
        };
        let selected = match &action {
            Action::SelectSession(name) => Some(self.kernel.open_session(name).await?.id),
            Action::Message(_) if !state.chats.contains_key(&target) => {
                Some(self.kernel.open_session("main").await?.id)
            }
            _ => None,
        };
        if let Some(session) = selected {
            state.chats.insert(target.clone(), session);
        }
        let events = if let Action::Message(message) = &action {
            let session = state.chats[&target];
            state.outbound.entry(session).or_default();
            vec![Event {
                id: EventId::new(),
                session,
                source: EventSource::Channel {
                    channel: self.identity.channel.clone(),
                    account: self.identity.account.clone(),
                    conversation: target.clone(),
                    sender: self.owner_id.clone(),
                },
                body: EventBody::UserMessage {
                    text: message.text.clone(),
                    images: message.images.clone(),
                    unviewable: message.unviewable.clone(),
                },
                received_at: self.clock.now(),
            }]
        } else {
            Vec::new()
        };
        self.commit(state, &events, None).await?;
        let response = match action {
            Action::Message(_) => {
                self.follow(self.state.chats[&target])?;
                return Ok(());
            }
            Action::Sessions => {
                let sessions = self.kernel.sessions().await?;
                let current = self.state.chats.get(&target);
                let names = sessions
                    .iter()
                    .map(|s| {
                        if Some(&s.id) == current {
                            format!("- **{}** (current)", s.name)
                        } else {
                            format!("- {}", s.name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if names.is_empty() {
                    "No Sessions yet. Send a message to open main.".into()
                } else {
                    names
                }
            }
            Action::SelectSession(name) => format!("Switched to {name}."),
            Action::Cancel => {
                let result = self
                    .state
                    .chats
                    .get(&target)
                    .map(|session| self.kernel.cancel(*session))
                    .transpose();
                match result {
                    Ok(Some(true)) => "Cancellation requested.".into(),
                    Ok(_) => "No active Run.".into(),
                    Err(error) => format!("Cancel failed: {error}"),
                }
            }
        };
        self.receipts.push_back(Receipt {
            target,
            text: response,
        });
        Ok(())
    }

    async fn next_delivery(&mut self) -> Result<(), ChannelError> {
        if self.in_flight.is_some() {
            return Ok(());
        }
        if let Some(Receipt { target, text }) = self.receipts.pop_front() {
            self.in_flight = Some(InFlight::Receipt(self.send(target, text)));
            return Ok(());
        }
        while let Some(session) = self.dirty.pop_first() {
            let mut cursor = self.state.outbound[&session].clone();
            let previous = cursor.processed;
            let entries = self.kernel.log(session, cursor.processed).await?;
            let text = projection::project(&mut cursor, &self.identity, session, entries);
            if let Some((delivery, mut text)) = text {
                let target = delivery.target.clone();
                if self.state.chats.get(&target) != Some(&session) {
                    let record = self
                        .kernel
                        .sessions()
                        .await?
                        .into_iter()
                        .find(|s| s.id == session)
                        .ok_or(KernelError::UnknownSession(session))?;
                    text = format!("[{}]\n\n{text}", record.name);
                }
                let mut state = self.state.clone();
                state.sending = Some(delivery.clone());
                self.commit(state, &[], None).await?;
                self.in_flight = Some(InFlight::Delivery(delivery, self.send(target, text)));
                break;
            }
            if previous != cursor.processed {
                let mut state = self.state.clone();
                state.outbound.insert(session, cursor);
                self.commit(state, &[], None).await?;
            }
        }
        Ok(())
    }

    fn send(&self, target: String, text: String) -> JoinHandle<transport::Sent> {
        tokio::spawn(transport::send(
            self.adapter.clone(),
            target,
            text,
            self.stop.clone(),
        ))
    }

    async fn sent(
        &mut self,
        flight: InFlight,
        result: Result<transport::Sent, tokio::task::JoinError>,
    ) -> Result<Option<Fault>, ChannelError> {
        let sent = result.map_err(|error| ChannelError::Task(error.to_string()))?;
        match flight {
            InFlight::Delivery(delivery, _) => self.settle(delivery, sent.outcome).await?,
            InFlight::Receipt(_) if !matches!(sent.outcome, Settlement::Ok) => {
                tracing::warn!(outcome = ?sent.outcome, "command receipt was not delivered");
            }
            InFlight::Receipt(_) => {}
        }
        Ok(sent.fatal)
    }

    async fn settle(
        &mut self,
        delivery: Delivery,
        outcome: Settlement,
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
            &[],
            Some(DeliverySettlement {
                delivery,
                outcome,
                at: self.clock.now(),
            }),
        )
        .await
    }
}
