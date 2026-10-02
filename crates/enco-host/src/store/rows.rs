pub(super) use crate::sqlite::{document, text, unsigned};
use enco_core::*;
use rusqlite::Row;
pub(super) fn session(row: &Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: text(row, 0)?,
        name: row.get(1)?,
        created_at: text(row, 2)?,
        binding: Binding {
            node: text(row, 3)?,
            epoch: Epoch(unsigned(row, 4)?),
        },
        profile: row.get(5)?,
    })
}

pub(super) fn entry(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        pos: LogPos {
            epoch: Epoch(unsigned(row, 0)?),
            seq: Seq(unsigned(row, 1)?),
        },
        at: text(row, 2)?,
        body: document(row, 3)?,
    })
}

pub(super) fn schedule(row: &Row<'_>) -> rusqlite::Result<Schedule> {
    let state: String = row.get(5)?;
    let state = match state.as_str() {
        "pending" => ScheduleState::Pending,
        "cancelled" => ScheduleState::Cancelled,
        "fired" => ScheduleState::Fired {
            event: text(row, 6)?,
        },
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                format!("unknown schedule state: {state:?}").into(),
            ));
        }
    };
    Ok(Schedule {
        id: text(row, 0)?,
        session: text(row, 1)?,
        due_at: text(row, 2)?,
        message: row.get(3)?,
        created_at: text(row, 4)?,
        state,
    })
}
