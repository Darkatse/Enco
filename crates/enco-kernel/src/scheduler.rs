//! The Scheduler owns no state: it reads the schedules' effective view, derives each next
//! occurrence, and hands a due reminder to the Inbox like any other input.
//!
//! Exactly once comes from identity: a reminder's Event ID follows from its schedule and planned
//! time, so a reminder sent again after a restart is a duplicate. Whether it may be written at
//! all is the Inbox's single write function's to say.
use crate::{
    Accepted, Clock, Store, StoreError, View, limits,
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
    sync::{Notify, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// What the Scheduler shares with the rest of the kernel.
#[derive(Default)]
pub(crate) struct SchedulerLink {
    /// Woken after a schedule is created or cancelled, so the Scheduler reads them again.
    pub changed: Notify,
    /// The failure that stopped the Scheduler, if any.
    pub stopped: Mutex<Option<String>>,
}

/// An active schedule with its derived next occurrence, for tools and administration.
#[derive(Debug, Clone)]
pub struct Scheduled {
    /// The schedule as its Session's facts give it.
    pub schedule: Schedule,
    /// Next planned delivery in UTC; this value is not persisted.
    pub next_due: DateTime<Utc>,
    /// Effective time zone for this view, including the owner's zone when implicit.
    pub timezone: Tz,
}

impl Scheduled {
    /// A schedule its creating call may record: meaningful text, a rule with an occurrence after
    /// its creation, and occurrences far enough apart; the first is its next.
    pub(crate) fn create(schedule: Schedule, owner_timezone: Tz) -> Result<Self, ScheduleError> {
        if schedule.message.trim().is_empty() {
            return Err(ScheduleError::EmptyMessage);
        }
        let rule = Rule::parse(&schedule.rule, owner_timezone)?;
        let next_due = rule.first_after(schedule.created_at)?;
        let mut previous = next_due;
        for _ in 1..limits::RECURRENCE_CHECK {
            let Some(next) = rule.next_after(previous)? else {
                break;
            };
            if next < previous + limits::MIN_RECURRENCE_INTERVAL {
                return Err(ScheduleError::TooFrequent);
            }
            previous = next;
        }
        Ok(Self {
            next_due,
            timezone: rule.timezone(),
            schedule,
        })
    }

    /// Its next occurrence follows the last one that reached its Session, else its creation.
    fn of(schedule: Schedule, owner_timezone: Tz) -> Result<Self, ScheduleError> {
        let (rule, next_due) = restore(&schedule, owner_timezone)?;
        Ok(Self {
            next_due,
            timezone: rule.timezone(),
            schedule,
        })
    }

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

/// The active ones among `schedules`, each with its next occurrence, in due order.
pub(crate) fn in_due_order(
    schedules: Vec<Schedule>,
    owner_timezone: Tz,
) -> Result<Vec<Scheduled>, ScheduleError> {
    let mut scheduled = schedules
        .into_iter()
        .filter(|schedule| schedule.state == ScheduleState::Active)
        .map(|schedule| Scheduled::of(schedule, owner_timezone))
        .collect::<Result<Vec<_>, _>>()?;
    scheduled.sort_by_key(|scheduled| (scheduled.next_due, scheduled.schedule.id));
    Ok(scheduled)
}

/// Schedule validation or persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// A reminder must contain meaningful text.
    #[error("reminder message is empty")]
    EmptyMessage,
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
    /// A recorded schedule could not be read back into occurrences.
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
}

/// A schedule's rule and next occurrence, which follows the last one that reached the Session,
/// else the schedule's creation; a failure names the schedule.
fn restore(
    schedule: &Schedule,
    owner_timezone: Tz,
) -> Result<(Rule, DateTime<Utc>), ScheduleError> {
    let context = |source| ScheduleError::Restore {
        id: schedule.id,
        source: Box::new(source),
    };
    let rule = Rule::parse(&schedule.rule, owner_timezone).map_err(context)?;
    let after = schedule
        .last
        .map_or(schedule.created_at, |last| last.due_at);
    let next = rule.first_after(after).map_err(context)?;
    Ok((rule, next))
}

/// Parsed rules are derived when read and stay local to their reader.
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

type Agenda = BTreeMap<(DateTime<Utc>, ScheduleId), AgendaEntry>;

/// Start reading schedules and delivering their reminders until shutdown.
pub(crate) fn start(
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    views: watch::Receiver<Arc<View>>,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>,
    link: Arc<SchedulerLink>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = run(&*store, &*clock, views, &sessions, &link, shutdown).await {
            *lock(&link.stopped) = Some(error.to_string());
            tracing::error!(%error, "Scheduler stopped");
        }
    })
}

async fn run(
    store: &dyn Store,
    clock: &dyn Clock,
    mut views: watch::Receiver<Arc<View>>,
    sessions: &Mutex<HashMap<SessionId, Arc<SessionHandle>>>,
    link: &SchedulerLink,
    shutdown: CancellationToken,
) -> Result<(), ScheduleError> {
    let mut timezone = views.borrow_and_update().config.kernel.timezone;
    let mut agenda = load(store, timezone).await?;
    let mut tick = tokio::time::interval(limits::SCHEDULER_TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return Ok(()),
            changed = views.changed() => {
                if changed.is_err() { return Ok(()); }
                let next = views.borrow_and_update().config.kernel.timezone;
                if next != timezone {
                    timezone = next;
                    agenda = load(store, timezone).await?;
                }
            },
            () = link.changed.notified() => agenda = load(store, timezone).await?,
            _ = tick.tick() => fire_due(store, clock.now(), &mut agenda, sessions).await?,
        }
    }
}

/// The active schedules' next occurrences, from their effective view.
async fn load(store: &dyn Store, timezone: Tz) -> Result<Agenda, ScheduleError> {
    let mut agenda = Agenda::new();
    for schedule in store.schedules(None).await? {
        if schedule.state != ScheduleState::Active {
            continue;
        }
        let (rule, next) = restore(&schedule, timezone)?;
        agenda.insert((next, schedule.id), AgendaEntry { schedule, rule });
    }
    Ok(agenda)
}

/// Deliver what is due; missed occurrences collapse into the latest, which says how many it
/// skipped.
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
        let ((mut due_at, id), entry) = entry.remove_entry();
        let mut skipped = 0;
        let mut next = entry.rule.next_after(due_at)?;
        while let Some(due) = next.filter(|at| *at <= now) {
            due_at = due;
            skipped += 1;
            next = entry.rule.next_after(due_at)?;
        }
        let schedule = &entry.schedule;
        let event = Event {
            id: EventId::reminder(id, due_at),
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
        match store
            .accept(std::slice::from_ref(&event), None)
            .await?
            .events[0]
        {
            Accepted::New => {
                if let Some(owner) = lock(sessions).get(&schedule.session) {
                    owner.wake.notify_one();
                }
            }
            // The occurrence is already in the Inbox or the Log.
            Accepted::Duplicate => {}
            // The schedule stopped, or its Session was archived: nothing more is due.
            Accepted::Inactive | Accepted::Archived => continue,
        }
        if let Some(next_due) = next {
            agenda.insert((next_due, id), entry);
        }
    }
    Ok(())
}
