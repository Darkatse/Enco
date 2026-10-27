//! The connection's actor, the only writer of its state: it accepts the owner's input, sends
//! replies and settles deliveries, one at a time.
pub(super) mod activity;
mod backtrack;
mod delivery;
mod input;
mod receipts;

use super::{projection, transport, *};
use enco_kernel::{ConnectionWrite, NewSession, StoreError};
use std::collections::BTreeSet;
use tokio::{
    sync::{broadcast, mpsc},
    task::{JoinHandle, JoinSet},
};

struct Connection {
    kernel: Arc<Kernel>,
    adapter: Arc<dyn Adapter>,
    identity: Identity,
    settings: ConnectionSettings,
    generation: GenerationId,
    state: State,
    menu_failure: Arc<Mutex<Option<String>>>,
    clock: Arc<dyn Clock>,
    stop: CancellationToken,
    readers: JoinSet<Result<(), ChannelError>>,
    watched: BTreeSet<SessionId>,
    dirty: BTreeSet<SessionId>,
    wake: mpsc::Sender<SessionId>,
    // Receipts of the reading being accepted, sent once its acknowledgments are done; they are
    // interface responses, not Log-derived deliveries.
    receipts: Vec<Receipt>,
    in_flight: Option<InFlight>,
    replies: JoinSet<Result<(), ChannelError>>,
    observations: BTreeMap<SessionId, activity::Observation>,
    activities: JoinSet<activity::Completed>,
    activity_busy: BTreeSet<RunId>,
}

/// What the connection says back about an accepted message.
enum Receipt {
    /// Answer the message holding this handle, with text or only to settle it.
    Answer {
        handle: String,
        text: Option<String>,
    },
    /// Say this in the conversation.
    Say { target: String, text: String },
}

struct InFlight {
    delivery: Delivery,
    task: JoinHandle<Result<transport::Sent, ChannelError>>,
}

/// Operational failure is observable state; only cleanup failures are returned by shutdown.
pub(super) struct Exit {
    pub stopped: Option<ChannelError>,
    pub shutdown: Result<(), ChannelError>,
}

pub(super) async fn run(
    kernel: Arc<Kernel>,
    adapter: Arc<dyn Adapter>,
    settings: ConnectionSettings,
    generation: GenerationId,
    clock: Arc<dyn Clock>,
    stop: CancellationToken,
    menu_failure: Arc<Mutex<Option<String>>>,
) -> Exit {
    let mut state = match restore(&kernel, adapter.as_ref()).await {
        Ok(state) => state,
        Err(error) => {
            return Exit {
                stopped: Some(error),
                shutdown: Ok(()),
            };
        }
    };
    for messages in state.context.values_mut() {
        let excess = messages.len().saturating_sub(settings.context);
        messages.drain(..excess);
    }
    state.context.retain(|_, messages| !messages.is_empty());
    let (wake, wakes) = mpsc::channel(crate::limits::CHANNEL_WAKE_CAPACITY);
    let mut connection = Connection {
        identity: adapter.identity(),
        kernel,
        adapter,
        settings,
        generation,
        menu_failure,
        state,
        clock,
        stop: stop.child_token(),
        readers: JoinSet::new(),
        watched: BTreeSet::new(),
        dirty: BTreeSet::new(),
        wake,
        receipts: Vec::new(),
        in_flight: None,
        replies: JoinSet::new(),
        observations: BTreeMap::new(),
        activities: JoinSet::new(),
        activity_busy: BTreeSet::new(),
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
            protocol: None,
            chats: BTreeMap::new(),
            context: BTreeMap::new(),
            everyone: BTreeSet::new(),
            outbound: BTreeMap::new(),
            sending: None,
            activity: BTreeMap::new(),
        },
    })
}

impl Connection {
    /// Run the connection until stopped, handling input, sends and wakes in order.
    ///
    /// First settle a delivery the last process left in flight and follow the Sessions this
    /// connection delivers for.
    async fn serve(&mut self, mut wakes: mpsc::Receiver<SessionId>) -> Result<(), ChannelError> {
        self.recover_delivery().await?;
        self.register_commands().await;
        let sessions: Vec<_> = self.state.outbound.keys().copied().collect();
        for session in sessions {
            self.follow(session)?;
            self.observe(session).await?;
        }
        let mut incoming = self.start_receiving();
        let mut ticks = tokio::time::interval(crate::limits::CHANNEL_ACTIVITY_INTERVAL);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if self.stop.is_cancelled() {
                return Ok(());
            }
            self.next_delivery().await?;
            self.refresh_activity(false).await?;
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Ok(()),
                result = async {
                    match &mut self.in_flight {
                        Some(flight) => (&mut flight.task).await,
                        None => std::future::pending().await,
                    }
                } => {
                    let flight = self.in_flight.take().ok_or_else(|| ChannelError::Task("send completed without an in-flight operation".into()))?;
                    if let Some(error) = self.sent(flight, result).await? { return Err(ChannelCallError::from(error).into()); }
                },
                Some(result) = self.readers.join_next() => {
                    result.map_err(|error| ChannelError::Task(error.to_string()))??;
                    return Err(ChannelError::Task("channel reader stopped unexpectedly".into()));
                }
                Some(batch) = incoming.recv() => {
                    match batch.change {
                        transport::Change::Message(update) => self.accept(*update).await?,
                        transport::Change::Checkpoint { protocol, accepted, acknowledged } => {
                            if let Some(protocol) = protocol {
                                let mut state = self.state.clone();
                                state.protocol = Some(protocol);
                                self.commit(state, None).await?;
                            }
                            if let Some(accepted) = accepted { let _ = accepted.send(()); }
                            self.answer(acknowledged);
                        }
                        transport::Change::Reset(failure) => {
                            tracing::warn!(connection = %self.identity.key(), code = %failure.code, reason = %failure.message, "channel protocol state reset; reading from initial state");
                            let mut state = self.state.clone();
                            state.protocol = None;
                            self.commit(state, None).await?;
                        }
                    }
                    let _ = batch.committed.send(transport::Policy { everyone: self.state.everyone.clone() });
                }
                Some(result) = self.activities.join_next() => {
                    let completed = result.map_err(|error| ChannelError::Task(error.to_string()))?;
                    self.activity_completed(completed).await?;
                }
                _ = ticks.tick() => self.refresh_activity(true).await?,
                Some(result) = self.replies.join_next() => {
                    result.map_err(|error| ChannelError::Task(error.to_string()))??;
                }
                Some(session) = wakes.recv() => { self.dirty.insert(session); self.observe(session).await?; }
            }
        }
    }

    fn start_receiving(&mut self) -> mpsc::Receiver<transport::Batch> {
        let (batches, incoming) = mpsc::channel(1);
        let (readings, received) = mpsc::unbounded_channel();
        let adapter = self.adapter.clone();
        let protocol = self.state.protocol.clone();
        let stop = self.stop.clone();
        let kernel = self.kernel.clone();
        let generation = self.generation;
        self.readers.spawn(async move {
            let pending = readings.clone();
            let result = health::call(
                &kernel,
                generation,
                adapter.receive(protocol, readings, stop.clone()),
            )
            .await;
            let unexpected = result.is_ok() && !stop.is_cancelled();
            stop.cancel();
            drop(pending);
            if unexpected {
                return Err(ChannelError::Task(
                    "channel receiver stopped unexpectedly".into(),
                ));
            }
            result
        });
        self.readers.spawn(
            transport::Reader {
                adapter: self.adapter.clone(),
                kernel: self.kernel.clone(),
                settings: self.settings.clone(),
                generation: self.generation,
                clock: self.clock.clone(),
            }
            .prepare(
                transport::Policy {
                    everyone: self.state.everyone.clone(),
                },
                received,
                batches,
                self.stop.clone(),
            ),
        );
        incoming
    }

    /// Signal stop, then wait for the send in flight and every reader to finish.
    async fn quiesce(mut self, result: Result<(), ChannelError>) -> Exit {
        self.stop.cancel();
        let mut exit = Exit {
            stopped: result.err(),
            shutdown: Ok(()),
        };
        if let Some(mut flight) = self.in_flight.take() {
            let result = (&mut flight.task).await;
            match self.sent(flight, result).await {
                Ok(Some(error)) => {
                    exit.stopped
                        .get_or_insert(ChannelCallError::from(error).into());
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
        while let Some(result) = self.activities.join_next().await {
            match result {
                Ok(completed) => {
                    if let Err(error) = self.activity_completed(completed).await {
                        exit.shutdown = Err(error);
                    }
                }
                Err(error) => exit.shutdown = Err(ChannelError::Task(error.to_string())),
            }
        }
        while let Some(result) = self.replies.join_next().await {
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

    /// Commit progress that accepts no protocol update: protocol state after a reset, cursors, or
    /// a delivery's final outcome.
    async fn commit(
        &mut self,
        state: State,
        settlement: Option<DeliverySettlement>,
    ) -> Result<(), ChannelError> {
        self.write(state, None, None, None, settlement).await
    }

    /// Accept one protocol update with the state it leads to, and the input and Session it brings.
    async fn commit_update(
        &mut self,
        state: State,
        update: String,
        event: Option<Event>,
        session: Option<NewSession>,
    ) -> Result<(), ChannelError> {
        self.write(state, Some(update), event, session, None).await
    }

    async fn write(
        &mut self,
        state: State,
        update: Option<String>,
        event: Option<Event>,
        session: Option<NewSession>,
        settlement: Option<DeliverySettlement>,
    ) -> Result<(), ChannelError> {
        self.kernel
            .accept(
                event.as_slice(),
                Some(&ConnectionWrite {
                    key: self.identity.key(),
                    state: serde_json::to_value(&state)?,
                    update,
                    session,
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
                    Ok(Entry { body: EntryBody::ToolCallStarted { .. } | EntryBody::ToolCallSettled { .. } | EntryBody::RunStarted { .. } | EntryBody::EventConsumed { .. } | EntryBody::RoundEnded { .. } | EntryBody::RunEnded { .. }, .. })
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

    /// The Sessions to choose from, marking the one this conversation is routed to.
    async fn sessions(&self, conversation: &str) -> Result<String, ChannelError> {
        let current = self.state.chats.get(conversation);
        let names = self
            .kernel
            .sessions()
            .await?
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
        Ok(if names.is_empty() {
            "No Sessions yet. Send a message to open main.".into()
        } else {
            names
        })
    }

    /// The Session's name, for telling the owner where something belongs.
    async fn session_name(&self, session: SessionId) -> Result<String, ChannelError> {
        let record = self
            .kernel
            .sessions()
            .await?
            .into_iter()
            .find(|s| s.id == session)
            .ok_or(KernelError::UnknownSession(session))?;
        Ok(record.name)
    }
}
