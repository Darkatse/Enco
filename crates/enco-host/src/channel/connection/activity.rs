//! Views of the Runs this connection shows, and the conversations it opens for child Sessions.
//!
//! A Run shows where `display` puts its Session; opening, creating, ending and moving views come
//! first. Each tick then sends a small batch: presences, which go where replies go, before
//! refreshes, which take turns.
use super::*;
use enco_kernel::{ChannelActivity, ChannelActivityUpdate, ChannelBackoff};

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::channel) struct Card {
    pub(super) session: SessionId,
    pub(super) target: String,
    /// Whether the Run's replies go to `target`; otherwise the card is only for display.
    pub(super) replies_here: bool,
    pub(super) creation: Creation,
}

/// The view requests in flight and the pacing that spaces them out.
#[derive(Default)]
pub(super) struct Views {
    /// Runs with a card request in flight.
    pub activity_busy: BTreeSet<RunId>,
    /// Runs with a presence in flight.
    pub presence_busy: BTreeSet<RunId>,
    /// Sessions whose conversation is being opened or closed.
    pub conversation_busy: BTreeSet<SessionId>,
    /// The last refresh sent; the next batch starts after it.
    pub refresh_cursor: Option<(DateTime<Utc>, RunId)>,
    /// View requests wait until this instant after the protocol refused one for now.
    pub retry_at: Option<tokio::time::Instant>,
    /// The wait after such a refusal when the protocol names none.
    pub backoff: ChannelBackoff,
}

pub(super) enum Operation {
    Open {
        session: SessionId,
        segments: Vec<String>,
    },
    Close {
        session: SessionId,
    },
    Activity {
        session: SessionId,
        run: RunId,
        replies_here: bool,
        shown: Option<String>,
        update: ChannelActivityUpdate,
    },
    Presence {
        run: RunId,
        view: ChannelActivity,
    },
}
pub(super) struct Request {
    pub target: String,
    pub operation: Operation,
}
pub(super) struct Completed {
    pub request: Request,
    pub result: Result<Option<String>, ChannelError>,
}

#[derive(Default)]
struct Schedule {
    priority: Vec<Request>,
    refreshes: Vec<((DateTime<Utc>, RunId), Request)>,
}

impl Connection {
    /// Send the view requests due now; nothing while the protocol asks to wait (`retry_at`).
    ///
    /// Opening and closing child conversations, ending the views of Runs that ended, removing
    /// views whose place changed, and creating views with their first presence come first. With
    /// `refresh`, on a tick, presences follow and refreshes fill the rest of the batch, taking
    /// turns by when their Run started.
    pub(super) async fn refresh_activity(&mut self, refresh: bool) -> Result<(), ChannelError> {
        if self.views.retry_at.is_some() {
            return Ok(());
        }
        let mut requests = self.conversation_requests();
        let moved = self.reconcile_cards(&mut requests).await?;
        let ended: Vec<_> = self
            .observations
            .iter()
            .flat_map(|(session, observation)| {
                observation
                    .runs
                    .values()
                    .filter_map(|run| run.ended.clone().map(|ended| (*session, run.id, ended)))
            })
            .collect();
        for (session, run, ended) in ended {
            if self.views.activity_busy.contains(&run) || moved.contains(&run) {
                continue;
            }
            let display = self
                .display(session)
                .map(|(target, replies_here)| (target.to_owned(), replies_here));
            let card = self.state.activity.get(&run).cloned();
            requests.extend(self.final_view(session, run, display, card, ended).await?);
        }
        let mut live = self.live_views(refresh, &moved).await?;
        requests.append(&mut live.priority);
        if requests
            .iter()
            .all(|request| matches!(request.operation, Operation::Presence { .. }))
        {
            // Presences count toward the batch; refreshes take what is left.
            let room = crate::limits::CHANNEL_ACTIVITY_BATCH.saturating_sub(requests.len());
            let after = self.views.refresh_cursor;
            live.refreshes
                .sort_by_key(|(key, _)| (after.is_some_and(|after| *key <= after), *key));
            for (key, request) in live.refreshes.into_iter().take(room) {
                self.views.refresh_cursor = Some(key);
                requests.push(request);
            }
        }
        for request in requests {
            self.start_view(request);
        }
        Ok(())
    }

    /// Where a Session shows on this connection, and whether its replies go there.
    ///
    /// That is where its replies go, or, while it works for its parent, its own conversation as
    /// long as that is still bound to it.
    pub(super) fn display(&self, session: SessionId) -> Option<(&str, bool)> {
        match self.observations.get(&session)?.destination.as_ref()? {
            ReplyDestination::Parent => match self.state.children.get(&session)? {
                Creation::Shown(conversation)
                    if self.state.chats.get(conversation) == Some(&session) =>
                {
                    Some((conversation, false))
                }
                Creation::Shown(_) | Creation::Forgone => None,
            },
            destination => {
                let conversation = projection::channel_target(destination, &self.identity)?;
                self.state
                    .reachable(conversation)
                    .then_some((conversation, true))
            }
        }
    }

    /// Open a conversation for each child that needs one now, and close the one still bound to
    /// each finished archived child.
    fn conversation_requests(&self) -> Vec<Request> {
        self.observations
            .iter()
            .filter(|(session, _)| !self.views.conversation_busy.contains(*session))
            .filter_map(|(session, observation)| {
                if !observation.archived {
                    return self.open_request(*session);
                }
                match self.state.children.get(session)? {
                    Creation::Shown(target)
                        if self.state.chats.get(target) == Some(session)
                            && self.finished(*session) =>
                    {
                        Some(Request {
                            target: target.clone(),
                            operation: Operation::Close { session: *session },
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// The parent's conversation that a conversation for `session` would open beside now, if any.
    ///
    /// That is a child working for its parent while the parent shows here, with no conversation
    /// opened or given up for it before.
    fn conversation_beside(&self, session: SessionId) -> Option<&str> {
        let observation = self.observations.get(&session)?;
        if !self.settings.show_children
            || observation.archived
            || self.state.children.contains_key(&session)
            || observation.destination != Some(ReplyDestination::Parent)
            || observation.runs.is_empty()
        {
            return None;
        }
        self.display(observation.child_of.as_ref()?.parent)
            .map(|(beside, _)| beside)
    }

    fn open_request(&self, session: SessionId) -> Option<Request> {
        let beside = self.conversation_beside(session)?;
        let name = &self.observations[&session].child_of.as_ref()?.name;
        Some(Request {
            target: beside.into(),
            operation: Operation::Open {
                session,
                segments: name.segments().map(str::to_owned).collect(),
            },
        })
    }

    /// Remove the cards of going Runs that no longer show where the card was made; returns the
    /// Runs whose old card is still going away.
    ///
    /// A card never shown is only forgotten. The card of a Run that ended stays where it is: the
    /// Run's view ends there.
    async fn reconcile_cards(
        &mut self,
        requests: &mut Vec<Request>,
    ) -> Result<BTreeSet<RunId>, ChannelError> {
        let obsolete: Vec<_> = self
            .state
            .activity
            .iter()
            .filter(|(run, card)| {
                let going = self
                    .observations
                    .get(&card.session)
                    .and_then(|observation| observation.runs.get(*run))
                    .is_some_and(|run| run.ended.is_none());
                going
                    && self.display(card.session) != Some((card.target.as_str(), card.replies_here))
            })
            .map(|(run, card)| (*run, card.clone()))
            .collect();
        let mut moved = BTreeSet::new();
        for (run, card) in obsolete {
            if self.views.activity_busy.contains(&run) {
                moved.insert(run);
                continue;
            }
            match card.creation {
                Creation::Shown(shown) => {
                    moved.insert(run);
                    requests.push(Request {
                        target: card.target,
                        operation: Operation::Activity {
                            session: card.session,
                            run,
                            replies_here: card.replies_here,
                            shown: Some(shown),
                            update: ChannelActivityUpdate::Remove,
                        },
                    });
                }
                Creation::Forgone => self.clear_card(run).await?,
            }
        }
        Ok(moved)
    }

    /// New cards and presences, as priorities, and refreshes of shown cards, keyed by when their
    /// Run started.
    async fn live_views(
        &mut self,
        refresh: bool,
        moved: &BTreeSet<RunId>,
    ) -> Result<Schedule, ChannelError> {
        let locations: BTreeMap<_, _> = self
            .observations
            .keys()
            .filter_map(|session| {
                self.display(*session)
                    .map(|(target, replies_here)| (*session, (target.to_owned(), replies_here)))
            })
            .collect();
        let mut schedule = Schedule::default();
        let mut plans = std::collections::HashMap::new();
        for (session, observation) in &mut self.observations {
            let Some((target, replies_here)) = locations.get(session) else {
                continue;
            };
            for run in observation.runs.values_mut() {
                if run.ended.is_some()
                    || self.views.activity_busy.contains(&run.id)
                    || moved.contains(&run.id)
                {
                    continue;
                }
                let card = self.state.activity.get(&run.id);
                let presence = *replies_here
                    && (refresh || card.is_none())
                    && !self.views.presence_busy.contains(&run.id);
                let shown = match card {
                    None => Some(None),
                    Some(Card {
                        creation: Creation::Shown(shown),
                        ..
                    }) if refresh => Some(Some(shown)),
                    Some(_) => None,
                };
                if !presence && shown.is_none() {
                    continue;
                }
                let view = run.view(&self.kernel, &mut plans, self.clock.now()).await?;
                if presence {
                    let request = Request {
                        target: target.clone(),
                        operation: Operation::Presence {
                            run: run.id,
                            view: view.clone(),
                        },
                    };
                    schedule.priority.push(request);
                }
                let Some(shown) = shown else {
                    continue;
                };
                let request = Request {
                    target: target.clone(),
                    operation: Operation::Activity {
                        session: *session,
                        run: run.id,
                        replies_here: *replies_here,
                        shown: shown.cloned(),
                        update: ChannelActivityUpdate::View(view),
                    },
                };
                if shown.is_none() {
                    schedule.priority.push(request);
                } else {
                    schedule.refreshes.push(((run.started, run.id), request));
                }
            }
        }
        Ok(schedule)
    }

    /// The request ending the view of a Run that ended; `None` when it ends without one.
    ///
    /// The view ends once deliveries have gone past the Run's end, so a restart knows from the
    /// delivery cursor which views have ended. Where the Run's replies go, the view is removed;
    /// elsewhere it is rewritten as the epilogue, or one is created when no card was made. A Run
    /// that shows nowhere ends without a request.
    async fn final_view(
        &mut self,
        session: SessionId,
        id: RunId,
        display: Option<(String, bool)>,
        card: Option<Card>,
        (pos, epilogue): (LogPos, enco_kernel::ChannelActivityEpilogue),
    ) -> Result<Option<Request>, ChannelError> {
        let location = card
            .as_ref()
            .map(|card| (card.target.clone(), card.replies_here))
            .or(display);
        let Some((target, replies_here)) = location else {
            // A child's conversation may still open for this Run; until it does or never will,
            // the view waits.
            if self.conversation_beside(session).is_none()
                && !self.views.conversation_busy.contains(&session)
            {
                self.finish_view(session, id).await?;
            }
            return Ok(None);
        };
        if self
            .state
            .outbound
            .get(&session)
            .and_then(|cursor| cursor.processed)
            < Some(pos)
        {
            return Ok(None);
        }
        let shown = match card.map(|card| card.creation) {
            Some(Creation::Shown(shown)) => Some(shown),
            None if !replies_here => None,
            _ => {
                self.finish_view(session, id).await?;
                return Ok(None);
            }
        };
        Ok(Some(Request {
            target,
            operation: Operation::Activity {
                session,
                run: id,
                replies_here,
                shown,
                update: if replies_here {
                    ChannelActivityUpdate::Remove
                } else {
                    ChannelActivityUpdate::Epilogue(epilogue)
                },
            },
        }))
    }

    pub(super) async fn finish_view(
        &mut self,
        session: SessionId,
        run: RunId,
    ) -> Result<(), ChannelError> {
        if let Some(observation) = self.observations.get_mut(&session) {
            observation.runs.remove(&run);
        }
        self.clear_card(run).await
    }

    pub(super) async fn clear_card(&mut self, run: RunId) -> Result<(), ChannelError> {
        let mut state = self.state.clone();
        if state.activity.remove(&run).is_some() {
            self.commit(state, None).await?;
        }
        Ok(())
    }

    fn start_view(&mut self, request: Request) {
        match &request.operation {
            Operation::Open { session, .. } | Operation::Close { session } => {
                self.views.conversation_busy.insert(*session);
            }
            Operation::Activity { run, .. } => {
                self.views.activity_busy.insert(*run);
            }
            Operation::Presence { run, .. } => {
                self.views.presence_busy.insert(*run);
            }
        }
        let adapter = self.adapter.clone();
        let kernel = self.kernel.clone();
        let generation = self.generation;
        self.activities.spawn(async move {
            let result = health::call(&kernel, generation, async {
                match &request.operation {
                    Operation::Open { segments, .. } => {
                        adapter.open_conversation(&request.target, segments).await
                    }
                    Operation::Close { .. } => adapter
                        .close_conversation(&request.target)
                        .await
                        .map(|()| None),
                    Operation::Activity {
                        replies_here,
                        shown,
                        update,
                        ..
                    } => adapter
                        .activity(&request.target, *replies_here, shown.as_deref(), update)
                        .await
                        .map(|sent| sent.and_then(|sent| sent.message)),
                    Operation::Presence { view, .. } => {
                        adapter.presence(&request.target, view).await.map(|()| None)
                    }
                }
            })
            .await;
            Completed { request, result }
        });
    }
}
