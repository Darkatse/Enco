//! Schedule rows, each the fold of its Session's own facts, written only by that Session's
//! commits.
//!
//! Readers see the effective view: the rows with the pending cancels and reminders applied. That
//! includes the Session's own scheduling calls and the Inbox's write precondition for Events
//! naming a schedule.
use super::{backend, encode, rows, sessions};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::{Connection, Transaction, params};

const COLUMNS: &str = "id,session_id,rule,message,created_at,state,last_due,last_event_id";

/// The schedules of every unarchived Session, or of one, in their effective view.
pub(super) fn list(
    connection: &Connection,
    session: Option<SessionId>,
) -> Result<Vec<Schedule>, StoreError> {
    let owners = connection
        .prepare(
            "SELECT DISTINCT session_id FROM schedules
             WHERE (?1 IS NULL OR session_id=?1)
               AND session_id IN (SELECT id FROM sessions WHERE archived_at IS NULL)
             ORDER BY session_id",
        )
        .map_err(backend)?
        .query_map([session.map(|id| id.to_string())], |row| rows::text(row, 0))
        .map_err(backend)?
        .collect::<Result<Vec<SessionId>, _>>()
        .map_err(backend)?;
    let mut all = Vec::new();
    for owner in owners {
        all.extend(effective(connection, owner)?.rows());
    }
    Ok(all)
}

/// One Session's materialized rows: the fold of its Log.
pub(super) fn committed(
    connection: &Connection,
    session: SessionId,
) -> Result<Vec<Schedule>, StoreError> {
    connection
        .prepare(&format!(
            "SELECT {COLUMNS} FROM schedules WHERE session_id=? ORDER BY id"
        ))
        .map_err(backend)?
        .query_map([session.to_string()], rows::schedule)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

/// The same fold that commits a consumed cancel or reminder, continued over the pending Inbox.
pub(super) fn effective(
    connection: &Connection,
    session: SessionId,
) -> Result<ScheduleFold, StoreError> {
    let mut fold = ScheduleFold::from_rows(session, committed(connection, session)?);
    for event in sessions::pending(connection, session)? {
        fold.consume(&event);
    }
    Ok(fold)
}

/// Fold the facts a Session's commit appends into its rows.
///
/// A fact about a schedule the Session never created fails the commit: no producer may record
/// one.
pub(super) fn apply(
    tx: &Transaction<'_>,
    session: SessionId,
    entries: &[Entry],
) -> Result<(), StoreError> {
    let before = committed(tx, session)?;
    let mut fold = ScheduleFold::from_rows(session, before.iter().cloned());
    for entry in entries {
        fold.apply(&entry.body);
    }
    if let Some(schedule) = fold.unknown().next() {
        return Err(StoreError::UnknownSchedule { session, schedule });
    }
    for row in fold.rows() {
        if before.contains(&row) {
            continue;
        }
        tx.execute(
            &format!(
                "INSERT INTO schedules({COLUMNS}) VALUES (?,?,?,?,?,?,?,?)
                 ON CONFLICT(id) DO UPDATE SET rule=excluded.rule,message=excluded.message,
                   created_at=excluded.created_at,state=excluded.state,
                   last_due=excluded.last_due,last_event_id=excluded.last_event_id"
            ),
            params![
                row.id.to_string(),
                row.session.to_string(),
                encode(&row.rule)?,
                row.message,
                timestamp(row.created_at),
                state_name(row.state),
                row.last.map(|last| timestamp(last.due_at)),
                row.last.map(|last| last.event.to_string()),
            ],
        )
        .map_err(backend)?;
    }
    Ok(())
}

fn state_name(state: ScheduleState) -> &'static str {
    match state {
        ScheduleState::Active => "active",
        ScheduleState::Done => "done",
        ScheduleState::Cancelled => "cancelled",
    }
}
