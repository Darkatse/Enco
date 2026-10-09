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
    Ok(Schedule {
        id: text(row, 0)?,
        session: text(row, 1)?,
        rule: document(row, 2)?,
        message: row.get(3)?,
        created_at: text(row, 4)?,
        state: text(row, 5)?,
        last: match row.get_ref(6)? {
            rusqlite::types::ValueRef::Null => None,
            _ => Some(Occurrence {
                due_at: text(row, 6)?,
                event: text(row, 7)?,
            }),
        },
    })
}
