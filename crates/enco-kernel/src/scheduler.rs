use crate::{
    Clock, Store, StoreError, limits,
    session::{SessionHandle, lock},
};
use croner::{
    Cron,
    errors::CronError,
    parser::{CronParser, Seconds, Year},
};
use enco_core::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// Commands to the sole owner of durable schedules.
#[derive(Clone)]
pub struct Schedules {
    tx: mpsc::Sender<Command>,
    stopped: Arc<Mutex<Option<String>>>,
}

/// An active schedule with its derived next occurrence, for tools and administration.
#[derive(Debug, Clone)]
pub struct Scheduled {
    /// Durable rule and last delivered occurrence.
    pub schedule: Schedule,
    /// Next planned delivery in UTC; this value is not persisted.
    pub next_due: DateTime<Utc>,
    /// Effective time zone for this view, including the owner's zone when implicit.
    pub timezone: Tz,
}

impl Scheduled {
    /// Shared tool and CLI view, with local offsets fixed when producing the result.
    /// The rule echoes the creation arguments; `timezone` is the effective zone.
    pub fn into_json(self) -> Value {
        let local = |at: DateTime<Utc>| at.with_timezone(&self.timezone).to_rfc3339();
        let schedule = self.schedule;
        let rule = match schedule.rule {
            ScheduleRule::Once { at } => json!({ "at": local(at) }),
            ScheduleRule::Cron {
                expr,
                timezone: None,
            } => json!({ "cron": expr }),
            ScheduleRule::Cron {
                expr,
                timezone: Some(timezone),
            } => json!({ "cron": expr, "timezone": timezone }),
        };
        json!({
            "id": schedule.id,
            "session": schedule.session,
            "rule": rule,
            "message": schedule.message,
            "timezone": self.timezone,
            "next_due": local(self.next_due),
            "last_due": schedule.last.map(|last| local(last.due_at)),
        })
    }
}

type Reply<T> = oneshot::Sender<Result<T, ScheduleError>>;

enum Command {
    Create {
        session: SessionId,
        rule: ScheduleRule,
        message: String,
        reply: Reply<Scheduled>,
    },
    List {
        session: Option<SessionId>,
        reply: Reply<Vec<Scheduled>>,
    },
    Cancel {
        id: ScheduleId,
        reply: Reply<bool>,
    },
}

/// Schedule validation or persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// A reminder must contain meaningful text.
    #[error("reminder message is empty")]
    EmptyMessage,
    /// There is no such destination Session.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// Cron parsing or evaluation failed.
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    /// The rule has no occurrence after the supplied instant.
    #[error("the rule has no occurrence after {}", .0.to_rfc3339())]
    NoOccurrenceAfter(DateTime<FixedOffset>),
    /// A likely typo would wake the Agent too frequently.
    #[error(
        "occurrences must be at least {} minutes apart",
        limits::MIN_RECURRENCE_INTERVAL.as_secs() / 60
    )]
    TooFrequent,
    /// A persisted schedule could not be restored.
    #[error("schedule {id}: {source}")]
    Restore {
        /// Schedule whose rule failed.
        id: ScheduleId,
        /// Parsing or occurrence failure.
        source: Box<ScheduleError>,
    },
    /// The durable authority failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The owner has stopped, with its reported failure when one exists.
    #[error("scheduler has stopped{}", .0.as_ref().map_or(String::new(), |reason| format!(": {reason}")))]
    Stopped(Option<String>),
}

impl Schedules {
    pub(crate) fn stopped(&self) -> Option<String> {
        lock(&self.stopped).clone()
    }

    /// Persist a schedule addressed to an existing Session.
    pub async fn create(
        &self,
        session: SessionId,
        rule: ScheduleRule,
        message: String,
    ) -> Result<Scheduled, ScheduleError> {
        self.ask(|reply| Command::Create {
            session,
            rule,
            message,
            reply,
        })
        .await
    }

    /// List active schedules in due order, optionally limited to one Session.
    pub async fn list(&self, session: Option<SessionId>) -> Result<Vec<Scheduled>, ScheduleError> {
        self.ask(|reply| Command::List { session, reply }).await
    }

    /// Stop future occurrences. Events already in the Inbox are unaffected.
    pub async fn cancel(&self, id: ScheduleId) -> Result<bool, ScheduleError> {
        self.ask(|reply| Command::Cancel { id, reply }).await
    }

    /// Send one command and wait for the owner's reply.
    async fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, ScheduleError> {
        let (reply, result) = oneshot::channel();
        let stopped = || ScheduleError::Stopped(self.stopped());
        self.tx.send(command(reply)).await.map_err(|_| stopped())?;
        result.await.map_err(|_| stopped())?
    }
}

/// Parsed rules are derived once on creation or startup and stay local to their owner.
enum Rule {
    Once { at: DateTime<Utc>, timezone: Tz },
    Cron { pattern: Box<Cron>, timezone: Tz },
}

impl Rule {
    fn parse(rule: &ScheduleRule, owner_timezone: Tz) -> Result<Self, ScheduleError> {
        match rule {
            ScheduleRule::Once { at } => Ok(Self::Once {
                at: *at,
                timezone: owner_timezone,
            }),
            ScheduleRule::Cron { expr, timezone } => {
                let pattern = CronParser::builder()
                    .seconds(Seconds::Disallowed)
                    .year(Year::Disallowed)
                    .build()
                    .parse(expr)
                    .map_err(|error| ScheduleError::InvalidCron(error.to_string()))?;
                Ok(Self::Cron {
                    pattern: Box::new(pattern),
                    timezone: timezone.unwrap_or(owner_timezone),
                })
            }
        }
    }

    fn timezone(&self) -> Tz {
        match self {
            Self::Once { timezone, .. } | Self::Cron { timezone, .. } => *timezone,
        }
    }

    /// The first occurrence after `after`; a rule without one cannot be scheduled.
    fn first_after(&self, after: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError> {
        self.next_after(after)?.ok_or_else(|| {
            ScheduleError::NoOccurrenceAfter(after.with_timezone(&self.timezone()).fixed_offset())
        })
    }

    fn next_after(&self, after: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, ScheduleError> {
        match self {
            Self::Once { at, .. } => Ok((*at > after).then_some(*at)),
            Self::Cron { pattern, timezone } => {
                match pattern.find_next_occurrence(&after.with_timezone(timezone), false) {
                    Ok(next) => Ok(Some(next.to_utc())),
                    Err(CronError::TimeSearchLimitExceeded) => Ok(None),
                    Err(error) => Err(ScheduleError::InvalidCron(error.to_string())),
                }
            }
        }
    }
}

struct AgendaEntry {
    schedule: Schedule,
    rule: Rule,
}

impl AgendaEntry {
    fn scheduled(&self, next_due: DateTime<Utc>) -> Scheduled {
        Scheduled {
            schedule: self.schedule.clone(),
            next_due,
            timezone: self.rule.timezone(),
        }
    }
}

type Agenda = BTreeMap<(DateTime<Utc>, ScheduleId), AgendaEntry>;

pub(crate) struct Scheduler {
    rx: mpsc::Receiver<Command>,
    stopped: Arc<Mutex<Option<String>>>,
}

impl Scheduler {
    pub fn channel() -> (Schedules, Self) {
        let (tx, rx) = mpsc::channel(limits::SCHEDULER_CHANNEL_CAPACITY);
        let stopped = Arc::new(Mutex::new(None));
        (
            Schedules {
                tx,
                stopped: stopped.clone(),
            },
            Self { rx, stopped },
        )
    }

    pub fn start(
        mut self,
        store: Arc<dyn Store>,
        clock: Arc<dyn Clock>,
        timezone: Tz,
        sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
        shutdown: CancellationToken,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(error) = self
                .run(&*store, &*clock, timezone, &sessions, shutdown)
                .await
            {
                *lock(&self.stopped) = Some(error.to_string());
                tracing::error!(%error, "Scheduler stopped");
            }
        })
    }

    async fn run(
        &mut self,
        store: &dyn Store,
        clock: &dyn Clock,
        timezone: Tz,
        sessions: &Mutex<HashMap<SessionId, Arc<SessionHandle>>>,
        shutdown: CancellationToken,
    ) -> Result<(), ScheduleError> {
        let mut agenda = Agenda::new();
        for schedule in store.schedules(Some(ScheduleState::Active)).await? {
            let context = |source| ScheduleError::Restore {
                id: schedule.id,
                source: Box::new(source),
            };
            let rule = Rule::parse(&schedule.rule, timezone).map_err(context)?;
            let after = schedule
                .last
                .map_or(schedule.created_at, |last| last.due_at);
            let next = rule.first_after(after).map_err(context)?;
            agenda.insert((next, schedule.id), AgendaEntry { schedule, rule });
        }
        let mut tick = tokio::time::interval(limits::SCHEDULER_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                command = self.rx.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    command.execute(store, clock, timezone, &mut agenda).await;
                },
                _ = tick.tick() => fire_due(store, clock.now(), &mut agenda, sessions).await?,
            }
        }
    }
}

impl Command {
    // Finish each accepted command before handling cancellation or the next command.
    async fn execute(
        self,
        store: &dyn Store,
        clock: &dyn Clock,
        timezone: Tz,
        agenda: &mut Agenda,
    ) {
        match self {
            Self::Create {
                session,
                rule,
                message,
                reply,
            } => {
                let result = create(store, clock.now(), timezone, session, rule, message).await;
                let result = result.map(|(next_due, entry)| {
                    let scheduled = entry.scheduled(next_due);
                    agenda.insert((next_due, entry.schedule.id), entry);
                    scheduled
                });
                let _ = reply.send(result);
            }
            Self::List { session, reply } => {
                let schedules = agenda
                    .iter()
                    .filter(|(_, entry)| session.is_none_or(|id| entry.schedule.session == id))
                    .map(|((next_due, _), entry)| entry.scheduled(*next_due))
                    .collect();
                let _ = reply.send(Ok(schedules));
            }
            Self::Cancel { id, reply } => {
                let result = store.cancel_schedule(id).await.map_err(ScheduleError::from);
                if matches!(result, Ok(true)) {
                    agenda.retain(|(_, schedule), _| *schedule != id);
                }
                let _ = reply.send(result);
            }
        }
    }
}

async fn create(
    store: &dyn Store,
    now: DateTime<Utc>,
    timezone: Tz,
    session: SessionId,
    rule: ScheduleRule,
    message: String,
) -> Result<(DateTime<Utc>, AgendaEntry), ScheduleError> {
    if message.trim().is_empty() {
        return Err(ScheduleError::EmptyMessage);
    }
    if !store.sessions().await?.iter().any(|s| s.id == session) {
        return Err(ScheduleError::UnknownSession(session));
    }
    let parsed = Rule::parse(&rule, timezone)?;
    let next_due = parsed.first_after(now)?;
    let mut previous = next_due;
    for _ in 1..limits::RECURRENCE_CHECK {
        let Some(next) = parsed.next_after(previous)? else {
            break;
        };
        if next < previous + limits::MIN_RECURRENCE_INTERVAL {
            return Err(ScheduleError::TooFrequent);
        }
        previous = next;
    }
    let schedule = Schedule {
        id: ScheduleId::new(),
        session,
        rule,
        message,
        created_at: now,
        state: ScheduleState::Active,
        last: None,
    };
    store.insert_schedule(&schedule).await?;
    Ok((
        next_due,
        AgendaEntry {
            schedule,
            rule: parsed,
        },
    ))
}

async fn fire_due(
    store: &dyn Store,
    now: DateTime<Utc>,
    agenda: &mut Agenda,
    sessions: &Mutex<HashMap<SessionId, Arc<SessionHandle>>>,
) -> Result<(), ScheduleError> {
    while let Some(entry) = agenda.first_entry() {
        if entry.key().0 > now {
            break;
        }
        let ((mut due_at, id), mut entry) = entry.remove_entry();
        let mut skipped = 0;
        let mut next = entry.rule.next_after(due_at)?;
        while let Some(due) = next.filter(|at| *at <= now) {
            due_at = due;
            skipped += 1;
            next = entry.rule.next_after(due_at)?;
        }
        let schedule = &mut entry.schedule;
        let event = Event {
            id: EventId::new(),
            session: schedule.session,
            source: EventSource::Scheduler,
            body: EventBody::Reminder {
                schedule: id,
                due_at,
                skipped,
                text: schedule.message.clone(),
            },
            received_at: now,
        };
        store
            .fire_schedule(
                schedule.last.map(|last| last.due_at),
                next.is_none(),
                &event,
            )
            .await?;
        schedule.last = Some(Occurrence {
            due_at,
            event: event.id,
        });
        if let Some(owner) = lock(sessions).get(&schedule.session) {
            owner.wake.notify_one();
        }
        if let Some(next_due) = next {
            agenda.insert((next_due, id), entry);
        }
    }
    Ok(())
}
