use crate::{
    Clock, ScheduleStateKind, Store, StoreError, limits,
    session::{SessionHandle, lock},
};
use enco_core::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// Commands to the sole owner of durable reminders.
#[derive(Clone)]
pub struct Schedules {
    tx: mpsc::Sender<Command>,
}

enum Command {
    Create {
        session: SessionId,
        due_at: DateTime<Utc>,
        message: String,
        reply: oneshot::Sender<Result<Schedule, ScheduleError>>,
    },
    List {
        session: Option<SessionId>,
        reply: oneshot::Sender<Result<Vec<Schedule>, ScheduleError>>,
    },
    Cancel {
        id: ScheduleId,
        reply: oneshot::Sender<Result<bool, ScheduleError>>,
    },
}

/// Reminder command validation or persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// The requested delivery time has already passed.
    #[error("due time {0} is not in the future")]
    InPast(DateTime<Utc>),
    /// A reminder must contain meaningful text.
    #[error("reminder message is empty")]
    EmptyMessage,
    /// There is no such destination Session.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// The durable authority failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The owner has stopped and did not accept this command.
    #[error("scheduler has stopped")]
    Stopped,
}

impl Schedules {
    /// Persist a reminder addressed to an existing Session.
    pub async fn create(
        &self,
        session: SessionId,
        due_at: DateTime<Utc>,
        message: String,
    ) -> Result<Schedule, ScheduleError> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(Command::Create {
                session,
                due_at,
                message,
                reply,
            })
            .await
            .map_err(|_| ScheduleError::Stopped)?;
        result.await.map_err(|_| ScheduleError::Stopped)?
    }

    /// List pending reminders, optionally limited to one Session.
    pub async fn list(&self, session: Option<SessionId>) -> Result<Vec<Schedule>, ScheduleError> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(Command::List { session, reply })
            .await
            .map_err(|_| ScheduleError::Stopped)?;
        result.await.map_err(|_| ScheduleError::Stopped)?
    }

    /// Cancel a pending reminder; false means it has fired, was cancelled, or is absent.
    pub async fn cancel(&self, id: ScheduleId) -> Result<bool, ScheduleError> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(Command::Cancel { id, reply })
            .await
            .map_err(|_| ScheduleError::Stopped)?;
        result.await.map_err(|_| ScheduleError::Stopped)?
    }
}

pub(crate) struct Scheduler {
    rx: mpsc::Receiver<Command>,
}

impl Scheduler {
    pub fn channel() -> (Schedules, Self) {
        let (tx, rx) = mpsc::channel(limits::SCHEDULER_CHANNEL_CAPACITY);
        (Schedules { tx }, Self { rx })
    }

    pub fn start(
        mut self,
        store: Arc<dyn Store>,
        clock: Arc<dyn Clock>,
        sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
        shutdown: CancellationToken,
    ) -> JoinHandle<Result<(), ScheduleError>> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(limits::SCHEDULER_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => return Ok(()),
                    command = self.rx.recv() => {
                        let Some(command) = command else { return Ok(()); };
                        command.execute(&*store, &*clock).await;
                    },
                    _ = tick.tick() => {
                        if let Err(error) = fire_due(&*store, &*clock, &sessions).await {
                            tracing::error!(%error, "Scheduler stopped");
                            return Err(error);
                        }
                    }
                }
            }
        })
    }
}

impl Command {
    // Finish each accepted command before handling cancellation or the next command.
    async fn execute(self, store: &dyn Store, clock: &dyn Clock) {
        match self {
            Self::Create {
                session,
                due_at,
                message,
                reply,
            } => {
                let result = create(store, clock, session, due_at, message).await;
                let _ = reply.send(result);
            }
            Self::List { session, reply } => {
                let result = store.schedules(Some(ScheduleStateKind::Pending)).await;
                let result = result
                    .map(|schedules| {
                        schedules
                            .into_iter()
                            .filter(|schedule| session.is_none_or(|id| schedule.session == id))
                            .collect()
                    })
                    .map_err(ScheduleError::from);
                let _ = reply.send(result);
            }
            Self::Cancel { id, reply } => {
                let result = store.cancel_schedule(id).await.map_err(ScheduleError::from);
                let _ = reply.send(result);
            }
        }
    }
}

async fn create(
    store: &dyn Store,
    clock: &dyn Clock,
    session: SessionId,
    due_at: DateTime<Utc>,
    message: String,
) -> Result<Schedule, ScheduleError> {
    let now = clock.now();
    if due_at <= now {
        return Err(ScheduleError::InPast(due_at));
    }
    if message.trim().is_empty() {
        return Err(ScheduleError::EmptyMessage);
    }
    if !store.sessions().await?.iter().any(|s| s.id == session) {
        return Err(ScheduleError::UnknownSession(session));
    }
    let schedule = Schedule {
        id: ScheduleId::new(),
        session,
        due_at,
        message,
        created_at: now,
        state: ScheduleState::Pending,
    };
    store.insert_schedule(&schedule).await?;
    Ok(schedule)
}

async fn fire_due(
    store: &dyn Store,
    clock: &dyn Clock,
    sessions: &Mutex<HashMap<SessionId, Arc<SessionHandle>>>,
) -> Result<(), ScheduleError> {
    let now = clock.now();
    for schedule in store
        .schedules(Some(ScheduleStateKind::Pending))
        .await?
        .into_iter()
        .filter(|s| s.due_at <= now)
    {
        let event = Event {
            id: EventId::new(),
            session: schedule.session,
            source: EventSource::Scheduler,
            body: EventBody::Reminder {
                schedule: schedule.id,
                due_at: schedule.due_at,
                text: schedule.message,
            },
            received_at: now,
        };
        store.fire_schedule(schedule.id, &event).await?;
        if let Some(owner) = lock(sessions).get(&schedule.session) {
            owner.wake.notify_one();
        }
    }
    Ok(())
}
