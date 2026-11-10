//! Schedules are a Session's own facts: what created and cancelled them, and which occurrences
//! reached the Session.
//!
//! The facts are folded by identity, so their order does not matter.
use crate::{DateTime, EntryBody, Event, EventBody, EventId, ScheduleId, SessionId, Tz, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A schedule of reminders to one Session, as its facts give it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// Identity used to cancel this schedule and link its delivered Events.
    pub id: ScheduleId,
    /// The Session whose facts define it, and to which its reminders go.
    pub session: SessionId,
    /// Rule from which occurrences are derived.
    pub rule: ScheduleRule,
    /// Reminder text delivered when due.
    pub message: String,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Lifetime state, derived from what is known about it.
    pub state: ScheduleState,
    /// The latest occurrence that reached the Session's Inbox.
    pub last: Option<Occurrence>,
}

/// One instant or a recurring pattern interpreted in local time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleRule {
    /// A single future instant.
    Once {
        /// Planned delivery time in UTC.
        at: DateTime<Utc>,
    },
    /// A five-field cron expression (minute, hour, day, month, weekday).
    Cron {
        /// Pattern as provided by the caller.
        expr: String,
        /// IANA time zone; absent means follow the owner's configured zone.
        timezone: Option<Tz>,
    },
}

/// An occurrence that reached the Session; the next one is derived rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    /// Planned time, independent of when the reminder was delivered.
    pub due_at: DateTime<Utc>,
    /// The reminder that carried it.
    pub event: EventId,
}

/// Schedule lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleState {
    /// Future occurrences may be delivered.
    Active,
    /// Its one occurrence reached the Session.
    Done,
    /// Cancelled; a cancel wins over any other fact.
    Cancelled,
}

/// A change a call made to its own Session's schedules, committed with its settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleChange {
    /// The call created a schedule.
    Created {
        /// The new schedule's identity.
        id: ScheduleId,
        /// Rule from which its occurrences are derived.
        rule: ScheduleRule,
        /// Reminder text delivered when due.
        message: String,
        /// Creation time; the first occurrence follows it.
        created_at: DateTime<Utc>,
    },
    /// The call cancelled one of the Session's schedules.
    Cancelled {
        /// The schedule it stopped.
        id: ScheduleId,
    },
}

/// What is known about one schedule; every fact only adds to it, so the order is irrelevant.
#[derive(Debug, Clone, Default, PartialEq)]
struct Known {
    created: Option<(ScheduleRule, String, DateTime<Utc>)>,
    cancelled: bool,
    last: Option<Occurrence>,
}

impl Known {
    fn remind(&mut self, occurrence: Occurrence) {
        if self.last.is_none_or(|last| last.due_at < occurrence.due_at) {
            self.last = Some(occurrence);
        }
    }
}

/// A Session's schedules, folded by identity from its facts: settlement changes, consumed
/// owner cancels and consumed reminders.
///
/// The same functions fold the Log into the committed view and the pending Inbox into the
/// effective view.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleFold {
    session: SessionId,
    known: BTreeMap<ScheduleId, Known>,
}

impl ScheduleFold {
    /// Nothing known yet.
    pub fn new(session: SessionId) -> Self {
        Self {
            session,
            known: BTreeMap::new(),
        }
    }

    /// Continue from materialized rows; a row keeps every fact the fold needs.
    pub fn from_rows(session: SessionId, rows: impl IntoIterator<Item = Schedule>) -> Self {
        let mut fold = Self::new(session);
        for row in rows {
            fold.known.insert(
                row.id,
                Known {
                    created: Some((row.rule, row.message, row.created_at)),
                    cancelled: row.state == ScheduleState::Cancelled,
                    last: row.last,
                },
            );
        }
        fold
    }

    /// Apply one fact of the Session's own Log.
    pub fn apply(&mut self, body: &EntryBody) {
        match body {
            EntryBody::EventConsumed { event } => self.consume(event),
            EntryBody::ToolCallSettled {
                schedule: Some(change),
                ..
            } => match change {
                ScheduleChange::Created {
                    id,
                    rule,
                    message,
                    created_at,
                } => {
                    self.known.entry(*id).or_default().created =
                        Some((rule.clone(), message.clone(), *created_at));
                }
                ScheduleChange::Cancelled { id } => {
                    self.known.entry(*id).or_default().cancelled = true;
                }
            },
            _ => {}
        }
    }

    /// Apply one input the Session consumed or will consume: an owner's cancel or a reminder.
    pub fn consume(&mut self, event: &Event) {
        match &event.body {
            EventBody::CancelSchedule { schedule } => {
                self.known.entry(*schedule).or_default().cancelled = true;
            }
            EventBody::Reminder {
                schedule, due_at, ..
            } => self.known.entry(*schedule).or_default().remind(Occurrence {
                due_at: *due_at,
                event: event.id,
            }),
            _ => {}
        }
    }

    /// Facts about schedules this Session never created, which no producer may record.
    pub fn unknown(&self) -> impl Iterator<Item = ScheduleId> + '_ {
        self.known
            .iter()
            .filter(|(_, known)| known.created.is_none())
            .map(|(id, _)| *id)
    }

    /// One schedule, if this Session created it.
    pub fn get(&self, id: ScheduleId) -> Option<Schedule> {
        self.row(id, self.known.get(&id)?)
    }

    /// Every schedule this Session created, in identity order.
    pub fn rows(&self) -> Vec<Schedule> {
        self.known
            .iter()
            .filter_map(|(id, known)| self.row(*id, known))
            .collect()
    }

    fn row(&self, id: ScheduleId, known: &Known) -> Option<Schedule> {
        let (rule, message, created_at) = known.created.clone()?;
        let state = if known.cancelled {
            ScheduleState::Cancelled
        } else if matches!(rule, ScheduleRule::Once { .. }) && known.last.is_some() {
            ScheduleState::Done
        } else {
            ScheduleState::Active
        };
        Some(Schedule {
            id,
            session: self.session,
            rule,
            message,
            created_at,
            state,
            last: known.last,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallId, EventSource, Settlement};

    fn settled(change: ScheduleChange) -> EntryBody {
        EntryBody::ToolCallSettled {
            call: CallId::new(),
            outcome: Settlement::Ok,
            content: String::new(),
            full: None,
            images: Vec::new(),
            outgoing: None,
            delegated: None,
            schedule: Some(change),
        }
    }

    fn event(session: SessionId, body: EventBody) -> Event {
        Event {
            id: EventId::new(),
            session,
            source: EventSource::Scheduler,
            body,
            received_at: Utc::now(),
        }
    }

    fn reminder(session: SessionId, schedule: ScheduleId, hour: u32) -> Event {
        let due_at = "2026-11-10T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
            + chrono::Duration::hours(i64::from(hour));
        let mut event = event(
            session,
            EventBody::Reminder {
                schedule,
                due_at,
                skipped: 0,
                text: "check".into(),
            },
        );
        event.id = EventId::reminder(schedule, due_at);
        event
    }

    fn created(id: ScheduleId, rule: ScheduleRule) -> EntryBody {
        settled(ScheduleChange::Created {
            id,
            rule,
            message: "check".into(),
            created_at: "2026-11-09T00:00:00Z".parse().unwrap(),
        })
    }

    /// One Session's facts about three schedules: created, reminded, cancelled by the Agent or
    /// by the owner, and a once-only schedule that is done.
    fn facts(session: SessionId) -> (Vec<EntryBody>, [ScheduleId; 3]) {
        let ids = [ScheduleId::new(), ScheduleId::new(), ScheduleId::new()];
        let cron = ScheduleRule::Cron {
            expr: "0 * * * *".into(),
            timezone: None,
        };
        let once = ScheduleRule::Once {
            at: "2026-11-10T01:00:00Z".parse().unwrap(),
        };
        let consumed = |event| EntryBody::EventConsumed { event };
        let facts = vec![
            created(ids[0], cron.clone()),
            created(ids[1], cron),
            created(ids[2], once),
            consumed(reminder(session, ids[0], 1)),
            consumed(reminder(session, ids[0], 3)),
            consumed(reminder(session, ids[0], 2)),
            settled(ScheduleChange::Cancelled { id: ids[0] }),
            consumed(event(
                session,
                EventBody::CancelSchedule { schedule: ids[1] },
            )),
            consumed(reminder(session, ids[2], 1)),
        ];
        (facts, ids)
    }

    fn fold(session: SessionId, facts: &[EntryBody]) -> Vec<Schedule> {
        let mut fold = ScheduleFold::new(session);
        for fact in facts {
            fold.apply(fact);
        }
        fold.rows()
    }

    #[test]
    fn the_fold_is_independent_of_the_order_of_the_facts() {
        let session = SessionId::new();
        let (facts, ids) = facts(session);
        let rows = fold(session, &facts);
        let states: Vec<_> = rows.iter().map(|row| (row.id, row.state)).collect();
        let mut expected = vec![
            (ids[0], ScheduleState::Cancelled),
            (ids[1], ScheduleState::Cancelled),
            (ids[2], ScheduleState::Done),
        ];
        expected.sort_by_key(|(id, _)| *id);
        assert_eq!(states, expected);
        let first = rows.iter().find(|row| row.id == ids[0]).unwrap();
        assert_eq!(
            first.last.unwrap().due_at.to_rfc3339(),
            "2026-11-10T03:00:00+00:00"
        );
        for rotation in 1..facts.len() {
            let mut shuffled = facts.clone();
            shuffled.rotate_left(rotation);
            shuffled.reverse();
            assert_eq!(fold(session, &shuffled), rows);
        }
        // A cancel repeated by both producers is the same fact.
        let mut twice = facts.clone();
        twice.push(settled(ScheduleChange::Cancelled { id: ids[1] }));
        assert_eq!(fold(session, &twice), rows);
    }

    /// Consuming what is pending turns the effective view into the committed one, and folding
    /// from materialized rows continues the fold from scratch.
    #[test]
    fn consuming_the_pending_inbox_commits_exactly_the_effective_view() {
        let session = SessionId::new();
        let (facts, ids) = facts(session);
        let mut committed = ScheduleFold::new(session);
        for fact in &facts[..3] {
            committed.apply(fact);
        }
        let pending = [
            reminder(session, ids[0], 1),
            event(session, EventBody::CancelSchedule { schedule: ids[1] }),
            reminder(session, ids[2], 1),
        ];
        let mut effective = ScheduleFold::from_rows(session, committed.rows());
        for event in &pending {
            effective.consume(event);
        }
        for event in &pending {
            committed.apply(&EntryBody::EventConsumed {
                event: event.clone(),
            });
        }
        assert_eq!(committed.rows(), effective.rows());
        assert_eq!(
            ScheduleFold::from_rows(session, committed.rows()).rows(),
            committed.rows()
        );
        assert_eq!(committed.unknown().count(), 0);
    }
}
