//! Progress views and presence follow their own Log cursor; the connection owns each card's
//! creation and cleanup, while presence keeps nothing.
use super::*;
use enco_kernel::{ChannelActivity, ChannelSent, ChannelToolActivity, ChannelToolState};

#[derive(Default)]
pub(super) struct Observation {
    cursor: Option<LogPos>,
    target: Option<String>,
    run: Option<ObservedRun>,
    /// Where Runs that have, or are getting, a card ended; the Log is the record, this a cache.
    ended: BTreeMap<RunId, LogPos>,
}
struct ObservedRun {
    id: RunId,
    started: DateTime<Utc>,
    tools: Vec<(CallId, ChannelToolActivity)>,
    ended: bool,
}
/// The one creation attempt a Run's card gets, handed over when the connection is replaced.
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::channel) struct Card {
    session: SessionId,
    target: String,
    creation: Creation,
}
#[derive(Clone, Serialize, Deserialize)]
enum Creation {
    /// The protocol's message for the card, refreshed and finally deleted.
    Shown(String),
    /// Failed, uncertain, unsupported or without a message; never attempted again for this Run.
    Abandoned,
}
#[derive(Clone, Copy)]
pub(super) enum Operation {
    Create {
        session: SessionId,
    },
    Refresh,
    Delete,
    /// A passing sign that the Run goes on; nothing of it is kept.
    Presence,
}
pub(super) struct Completed {
    run: RunId,
    operation: Operation,
    target: String,
    result: Result<Option<ChannelSent>, ChannelError>,
}

impl Connection {
    pub(super) async fn observe(&mut self, session: SessionId) -> Result<(), ChannelError> {
        let observation = self.observations.entry(session).or_default();
        for entry in self.kernel.log(session, observation.cursor).await? {
            observation.cursor = Some(entry.pos);
            match entry.body {
                EntryBody::EventConsumed { event } => {
                    projection::route(&mut observation.target, &self.identity, event.source)
                }
                EntryBody::RunStarted { run } => {
                    observation.run = Some(ObservedRun {
                        id: run,
                        started: entry.at,
                        tools: Vec::new(),
                        ended: false,
                    })
                }
                EntryBody::RunEnded { run, .. } => {
                    if let Some(current) = &mut observation.run
                        && current.id == run
                    {
                        current.ended = true;
                    }
                    if self.state.activity.contains_key(&run) || self.activity_busy.contains(&run) {
                        observation.ended.insert(run, entry.pos);
                    }
                }
                EntryBody::ToolCallStarted {
                    call, capability, ..
                } => {
                    if let Some(run) = &mut observation.run {
                        run.tools.push((
                            call,
                            ChannelToolActivity {
                                name: capability.name,
                                state: ChannelToolState::Running,
                            },
                        ));
                        let excess = run
                            .tools
                            .len()
                            .saturating_sub(crate::limits::CHANNEL_ACTIVITY_TOOLS);
                        run.tools.drain(..excess);
                    }
                }
                EntryBody::ToolCallSettled { call, outcome, .. } => {
                    if let Some(run) = &mut observation.run
                        && let Some((_, tool)) = run.tools.iter_mut().find(|(id, _)| *id == call)
                    {
                        tool.state = match outcome {
                            Settlement::Ok => ChannelToolState::Ok,
                            Settlement::Failed { .. } => ChannelToolState::Failed,
                            Settlement::Unknown { .. } => ChannelToolState::Unknown,
                        };
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Create cards for new Runs, refresh shown ones on a tick, and remove those whose Run ended
    /// and whose replies have been delivered.
    pub(super) async fn refresh_activity(&mut self, refresh: bool) -> Result<(), ChannelError> {
        self.send_presence(refresh);
        let mut requests = Vec::new();
        for (session, observation) in &self.observations {
            let Some(run) = &observation.run else {
                continue;
            };
            if run.ended || self.activity_busy.contains(&run.id) {
                continue;
            }
            let (operation, target, shown) = match self.state.activity.get(&run.id) {
                None => match &observation.target {
                    Some(target) => (Operation::Create { session: *session }, target, None),
                    None => continue,
                },
                Some(Card {
                    target,
                    creation: Creation::Shown(shown),
                    ..
                }) if refresh => (Operation::Refresh, target, Some(shown.clone())),
                Some(_) => continue,
            };
            let view = ChannelActivity {
                elapsed_seconds: u32::try_from(
                    (self.clock.now() - run.started).num_seconds().max(0),
                )
                .unwrap_or(u32::MAX),
                tools: run.tools.iter().map(|(_, tool)| tool.clone()).collect(),
            };
            requests.push((run.id, operation, target.clone(), shown, Some(view)));
        }
        let mut finished = Vec::new();
        for (run, card) in &self.state.activity {
            let delivered = self
                .observations
                .get(&card.session)
                .and_then(|observation| observation.ended.get(run))
                .is_some_and(|ended| self.state.outbound[&card.session].processed >= Some(*ended));
            if !delivered || self.activity_busy.contains(run) {
                continue;
            }
            match &card.creation {
                Creation::Shown(shown) => requests.push((
                    *run,
                    Operation::Delete,
                    card.target.clone(),
                    Some(shown.clone()),
                    None,
                )),
                Creation::Abandoned => finished.push((*run, card.session)),
            }
        }
        if !finished.is_empty() {
            let mut state = self.state.clone();
            for (run, session) in finished {
                state.activity.remove(&run);
                self.forget_end(session, run);
            }
            self.commit(state, None).await?;
        }
        for (run, operation, target, shown, view) in requests {
            self.activity_busy.insert(run);
            let adapter = self.adapter.clone();
            let kernel = self.kernel.clone();
            let generation = self.generation;
            self.activities.spawn(async move {
                Completed {
                    run,
                    operation,
                    target: target.clone(),
                    result: health::call(
                        &kernel,
                        generation,
                        adapter.activity(&target, shown.as_deref(), view.as_ref()),
                    )
                    .await,
                }
            });
        }
        Ok(())
    }

    /// Show each Run in progress as it starts and on every tick, one request at a time per Run.
    fn send_presence(&mut self, refresh: bool) {
        let mut targets = Vec::new();
        for observation in self.observations.values() {
            let Some(run) = &observation.run else {
                continue;
            };
            let card = self.state.activity.get(&run.id);
            let starting = card.is_none() && !self.activity_busy.contains(&run.id);
            if run.ended || !(refresh || starting) || self.presence_busy.contains(&run.id) {
                continue;
            }
            if let Some(target) = card
                .map(|card| &card.target)
                .or(observation.target.as_ref())
            {
                targets.push((run.id, target.clone()));
            }
        }
        for (run, target) in targets {
            self.presence_busy.insert(run);
            let adapter = self.adapter.clone();
            let kernel = self.kernel.clone();
            let generation = self.generation;
            self.activities.spawn(async move {
                Completed {
                    run,
                    operation: Operation::Presence,
                    target: target.clone(),
                    result: health::call(&kernel, generation, adapter.presence(&target))
                        .await
                        .map(|()| None),
                }
            });
        }
    }

    pub(super) async fn activity_completed(
        &mut self,
        completed: Completed,
    ) -> Result<(), ChannelError> {
        let Completed {
            run,
            operation,
            target,
            result,
        } = completed;
        match operation {
            Operation::Presence => self.presence_busy.remove(&run),
            Operation::Create { .. } | Operation::Refresh | Operation::Delete => {
                self.activity_busy.remove(&run)
            }
        };
        let result = match result {
            Err(error @ ChannelError::Protocol(ChannelCallError::Protocol(_))) => {
                tracing::warn!(%error, "channel activity request failed");
                None
            }
            Err(error) => return Err(error),
            Ok(sent) => Some(sent),
        };
        match operation {
            Operation::Create { session } => {
                let creation = match result {
                    Some(Some(ChannelSent {
                        message: Some(shown),
                        ..
                    })) => Creation::Shown(shown),
                    Some(_) | None => Creation::Abandoned,
                };
                let mut state = self.state.clone();
                state.activity.insert(
                    run,
                    Card {
                        session,
                        target,
                        creation,
                    },
                );
                self.commit(state, None).await
            }
            Operation::Delete => {
                let mut state = self.state.clone();
                if let Some(card) = state.activity.remove(&run) {
                    self.forget_end(card.session, run);
                }
                self.commit(state, None).await
            }
            Operation::Refresh | Operation::Presence => Ok(()),
        }
    }

    fn forget_end(&mut self, session: SessionId, run: RunId) {
        if let Some(observation) = self.observations.get_mut(&session) {
            observation.ended.remove(&run);
        }
    }
}
