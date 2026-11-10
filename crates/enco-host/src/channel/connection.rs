//! The connection's actor, the only writer of its state: it accepts the owner's input, sends
//! replies and settles deliveries, one at a time.
pub(super) mod activity;
mod backtrack;
mod conversation;
mod delivery;
mod input;
mod lifecycle;
mod observation;
mod receipts;
mod status;
mod view_results;

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
    pacing: Arc<transport::PacingGate>,
    identity: Identity,
    settings: ConnectionSettings,
    code: CodeRef,
    state: State,
    menu_failure: Arc<Mutex<Option<String>>>,
    clock: Arc<dyn Clock>,
    build: crate::status::HostBuild,
    stop: CancellationToken,
    readers: JoinSet<Result<Option<SessionId>, ChannelError>>,
    watched: BTreeSet<SessionId>,
    dirty: BTreeSet<SessionId>,
    wake: mpsc::Sender<SessionId>,
    // Receipts of the reading being accepted, sent once its acknowledgments are done; they are
    // interface responses, not Log-derived deliveries.
    receipts: Vec<Receipt>,
    in_flight: Option<InFlight>,
    replies: JoinSet<Result<(), ChannelError>>,
    observations: BTreeMap<SessionId, observation::Observation>,
    activities: JoinSet<activity::Completed>,
    views: activity::Views,
}

impl State {
    /// Whether replies and views may still go to `conversation`, that is whether it has a route.
    ///
    /// Every conversation that gave input has one; `/session` moves it to another Session. A
    /// conversation loses it only when the protocol says it is gone (`forget`), or when the
    /// conversation opened for an archived child is put away.
    fn reachable(&self, conversation: &str) -> bool {
        self.chats.contains_key(conversation)
    }

    /// Forget a conversation the protocol says no longer exists: its route, its cards, and what
    /// was kept for it as a group.
    fn forget(&mut self, conversation: &str) {
        self.chats.remove(conversation);
        self.activity.retain(|_, card| card.target != conversation);
        self.everyone.remove(conversation);
        self.context.remove(conversation);
    }
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

/// Inputs fixed for the lifetime of this connection's actor.
pub(super) struct Start {
    pub kernel: Arc<Kernel>,
    pub adapter: Arc<dyn Adapter>,
    pub settings: ConnectionSettings,
    pub code: CodeRef,
    pub clock: Arc<dyn Clock>,
    pub stop: CancellationToken,
    pub menu_failure: Arc<Mutex<Option<String>>>,
    pub build: crate::status::HostBuild,
}

pub(super) async fn run(start: Start) -> Exit {
    let Start {
        kernel,
        adapter,
        settings,
        code,
        clock,
        stop,
        menu_failure,
        build,
    } = start;
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
        pacing: Arc::new(transport::PacingGate::new(adapter.pacing())),
        kernel,
        adapter,
        settings,
        code,
        menu_failure,
        state,
        clock,
        build,
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
        views: activity::Views::default(),
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
            children: BTreeMap::new(),
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
            self.follow(session).await?;
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
            self.retire().await?;
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
                    match result.map_err(|error| ChannelError::Task(error.to_string()))?? {
                        Some(session) => {
                            self.watched.remove(&session);
                            self.observations.entry(session).or_default().archived = true;
                            self.observe(session).await?;
                            // Drain the final facts through the usual projection, then forget the wake.
                            self.dirty.insert(session);
                        }
                        None => return Err(ChannelError::Task("channel reader stopped unexpectedly".into())),
                    }
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
                    let _ = batch.committed.send(transport::Policy::from(&self.state));
                }
                Some(result) = self.activities.join_next() => {
                    let completed = result.map_err(|error| ChannelError::Task(error.to_string()))?;
                    self.activity_completed(completed).await?;
                }
                _ = ticks.tick() => self.refresh_activity(true).await?,
                _ = async {
                    match self.views.retry_at {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.views.retry_at = None,
                Some(result) = self.replies.join_next() => {
                    result.map_err(|error| ChannelError::Task(error.to_string()))??;
                }
                Some(session) = wakes.recv() => {
                    if self.watched.contains(&session) {
                        self.dirty.insert(session);
                        self.observe(session).await?;
                    }
                }
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
        let code = self.code.clone();
        self.readers.spawn(async move {
            let pending = readings.clone();
            let result = health::call(
                &kernel,
                &code,
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
            result.map(|()| None)
        });
        let preparation = transport::Reader {
            adapter: self.adapter.clone(),
            kernel: self.kernel.clone(),
            settings: self.settings.clone(),
            code: self.code.clone(),
            clock: self.clock.clone(),
        }
        .prepare(
            transport::Policy::from(&self.state),
            received,
            batches,
            self.stop.clone(),
        );
        self.readers
            .spawn(async move { preparation.await.map(|()| None) });
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
                Ok(Ok(_)) => {}
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
        self.write(state, None, None, None, settlement)
            .await
            .map(drop)
    }

    /// Accept one protocol update with the state it leads to, and the input and Session it brings;
    /// returns how the input was accepted.
    async fn commit_update(
        &mut self,
        state: State,
        update: String,
        event: Option<Event>,
        session: Option<NewSession>,
    ) -> Result<Option<Accepted>, ChannelError> {
        let accepted = self
            .write(state, Some(update), event, session, None)
            .await?;
        Ok(accepted.first().copied())
    }

    async fn write(
        &mut self,
        state: State,
        update: Option<String>,
        event: Option<Event>,
        session: Option<NewSession>,
        settlement: Option<DeliverySettlement>,
    ) -> Result<Vec<Accepted>, ChannelError> {
        let accepted = self
            .kernel
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
        Ok(accepted)
    }

    async fn follow(&mut self, session: SessionId) -> Result<(), ChannelError> {
        if self.watched.contains(&session) {
            return Ok(());
        }
        // Subscribe before marking dirty; every commit is covered by Log or a later wake.
        let mut entries = match self.kernel.subscribe(session).await {
            Ok(entries) => entries,
            // One stopped Session goes unshown and undelivered until it runs again; the
            // connection keeps serving the others.
            Err(KernelError::SessionStopped(_, reason)) => {
                tracing::warn!(%session, %reason, "channel does not follow a stopped Session");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        self.watched.insert(session);
        self.dirty.insert(session);
        let wake = self.wake.clone();
        let stop = self.stop.clone();
        let kernel = self.kernel.clone();
        self.readers.spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = stop.cancelled() => return Ok(None),
                    event = entries.recv() => event,
                };
                match event {
                    Ok(Entry { body: EntryBody::ToolCallStarted { .. } | EntryBody::ToolCallSettled { .. } | EntryBody::RunStarted { .. } | EntryBody::EventConsumed { .. } | EntryBody::RoundEnded { .. } | EntryBody::RunEnded { .. }, .. })
                    | Err(broadcast::error::RecvError::Lagged(_)) => {
                        tokio::select! {
                            _ = stop.cancelled() => return Ok(None),
                            result = wake.send(session) => result.map_err(|_| ChannelError::Task("connection stopped".into()))?,
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if kernel.session(session).await?.archived_at.is_some() {
                            return Ok(Some(session));
                        }
                        return Err(ChannelError::Task(format!("Session {session} subscription closed")));
                    },
                    _ => {},
                }
            }
        });
        Ok(())
    }

    /// The Sessions to choose from, marking the one this conversation is routed to.
    async fn sessions(&self, conversation: &str) -> Result<String, ChannelError> {
        let current = self.state.chats.get(conversation);
        let mut sessions: Vec<_> = self
            .kernel
            .sessions()
            .await?
            .into_iter()
            .filter(|s| s.archived_at.is_none())
            .collect();
        sessions.sort_by(|a, b| a.name.cmp(&b.name));
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
        Ok(if names.is_empty() {
            "No Sessions yet. Send a message to open main.".into()
        } else {
            names
        })
    }
}
