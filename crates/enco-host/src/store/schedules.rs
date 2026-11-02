//! Reminder schedules: creating, cancelling and listing them, and firing one in the transaction
//! that delivers its Reminder.
use super::{backend, encode, rows};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{Accepted, StoreError};
use rusqlite::{Connection, TransactionBehavior, params};

const COLUMNS: &str = "id,session_id,rule,message,created_at,state,last_due,last_event_id";

fn state_name(state: ScheduleState) -> &'static str {
    match state {
        ScheduleState::Active => "active",
        ScheduleState::Done => "done",
        ScheduleState::Cancelled => "cancelled",
    }
}

pub(super) fn insert(connection: &Connection, schedule: &Schedule) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO schedules VALUES (?,?,?,?,?,?,?,?)",
            params![
                schedule.id.to_string(),
                schedule.session.to_string(),
                encode(&schedule.rule)?,
                schedule.message,
                timestamp(schedule.created_at),
                state_name(schedule.state),
                schedule.last.map(|last| timestamp(last.due_at)),
                schedule.last.map(|last| last.event.to_string()),
            ],
        )
        .map_err(backend)?;
    Ok(())
}

pub(super) fn cancel(connection: &Connection, id: ScheduleId) -> Result<bool, StoreError> {
    connection
        .execute(
            "UPDATE schedules SET state='cancelled' WHERE id=? AND state='active'",
            [id.to_string()],
        )
        .map(|n| n > 0)
        .map_err(backend)
}

pub(super) fn list(
    connection: &Connection,
    state: Option<ScheduleState>,
) -> Result<Vec<Schedule>, StoreError> {
    let filter = state.map(state_name);
    let mut query = connection
        .prepare(&format!(
            "SELECT {COLUMNS}
             FROM schedules
             WHERE (?1 IS NULL OR state=?1)
               AND (?1 IS NOT 'active' OR EXISTS (SELECT 1 FROM sessions s WHERE s.id=schedules.session_id AND s.archived_at IS NULL))
             ORDER BY created_at,id"
        ))
        .map_err(backend)?;
    query
        .query_map(params![filter], rows::schedule)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

/// The Scheduler's commit: advance the occurrence and deliver its Reminder in one transaction.
///
/// A trigger therefore happens exactly once, whichever side of the commit a crash falls on.
pub(super) fn fire(
    connection: &mut Connection,
    expected_last: Option<DateTime<Utc>>,
    done: bool,
    event: &Event,
) -> Result<Accepted, StoreError> {
    let (id, due_at) = match &event.body {
        EventBody::Reminder {
            schedule, due_at, ..
        } => (*schedule, *due_at),
        _ => {
            return Err(StoreError::Inbox("event is not a Reminder".into()));
        }
    };
    let events = super::accept::encode_events(std::slice::from_ref(event))?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let active: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM schedules WHERE id=? AND state='active' AND last_due IS ?)",
        params![id.to_string(), expected_last.map(timestamp)], |row| row.get(0),
    ).map_err(backend)?;
    if !active {
        return Err(StoreError::ScheduleNotActive(id));
    }
    match super::accept::events_in(&tx, &events)?[0] {
        Accepted::Archived => return Ok(Accepted::Archived),
        Accepted::Duplicate => {
            return Err(StoreError::Inbox(format!(
                "reminder {} was already accepted",
                event.id
            )));
        }
        Accepted::New => {}
    }
    tx.execute(
        "UPDATE schedules SET last_due=?,last_event_id=?,state=? WHERE id=?",
        params![
            timestamp(due_at),
            event.id.to_string(),
            state_name(if done {
                ScheduleState::Done
            } else {
                ScheduleState::Active
            }),
            id.to_string()
        ],
    )
    .map_err(backend)?;
    tx.commit().map_err(backend)?;
    Ok(Accepted::New)
}
