//! The facts version 24 folds Session and schedule rows from, and the check that folding them
//! gives back the rows version 23 kept.
//!
//! The facts are the Logs in their new form, root creation records, archives, and schedule
//! changes recorded where they belong.
use super::documents::{Records, Rewrite, creation, entry_origins};
use crate::sqlite::timestamp;
use crate::store::{backend, blob_path, content, encode, log, rows, schedules, sessions, signed};
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// One Log entry and whether the migration changed or added it.
struct Row {
    entry: Entry,
    changed: bool,
    added: bool,
}

/// Every Session's Log in its version 24 form.
pub(super) struct Logs(BTreeMap<SessionId, Vec<Row>>);

impl Logs {
    /// Read each Log, rewriting each entry that the format changed; every entry must then decode,
    /// and its origins follow their moved records.
    pub fn read(
        connection: &Connection,
        rewrite: &Rewrite<'_>,
        records: &mut Records<'_>,
    ) -> Result<Self, StoreError> {
        let rows = connection
            .prepare("SELECT session_id,epoch,seq,at,body FROM log ORDER BY session_id,epoch,seq")
            .map_err(backend)?
            .query_map([], |row| {
                Ok((
                    rows::text::<SessionId>(row, 0)?,
                    LogPos {
                        epoch: Epoch(rows::unsigned(row, 1)?),
                        seq: Seq(rows::unsigned(row, 2)?),
                    },
                    rows::text::<DateTime<Utc>>(row, 3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        let mut logs: BTreeMap<SessionId, Vec<Row>> = BTreeMap::new();
        for (session, pos, at, body) in rows {
            let original: serde_json::Value = serde_json::from_str(&body).map_err(backend)?;
            let mut value = original.clone();
            rewrite.entry(&mut value)?;
            let mut body = serde_json::from_value(value.clone())
                .map_err(|error| backend(format!("Session {session} at {pos:?}: {error}")))?;
            let followed = records.follow(entry_origins(&mut body))?;
            logs.entry(session).or_default().push(Row {
                entry: Entry { pos, at, body },
                changed: followed || value != original,
                added: false,
            });
        }
        Ok(Self(logs))
    }

    /// Write the changed and added entries back.
    pub fn write(&self, connection: &Connection) -> Result<(), StoreError> {
        for (session, rows) in &self.0 {
            for row in rows.iter().filter(|row| row.changed || row.added) {
                let Entry { pos, at, body } = &row.entry;
                let key = (
                    session.to_string(),
                    signed(pos.epoch.0)?,
                    signed(pos.seq.0)?,
                );
                if row.added {
                    connection
                        .execute(
                            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,?,?,?,?)",
                            params![key.0, key.1, key.2, timestamp(*at), encode(body)?],
                        )
                        .map_err(backend)?;
                } else {
                    connection
                        .execute(
                            "UPDATE log SET body=? WHERE session_id=? AND epoch=? AND seq=?",
                            params![encode(body)?, key.0, key.1, key.2],
                        )
                        .map_err(backend)?;
                }
            }
        }
        Ok(())
    }

    /// Materialize every Session's schedules by the function a commit uses.
    pub fn materialize(&self, connection: &rusqlite::Transaction<'_>) -> Result<(), StoreError> {
        connection
            .execute("DELETE FROM schedules", [])
            .map_err(backend)?;
        for (session, rows) in &self.0 {
            let entries: Vec<Entry> = rows.iter().map(|row| row.entry.clone()).collect();
            schedules::apply(connection, *session, &entries)?;
        }
        Ok(())
    }

    /// Record each schedule's facts in its Session's Log.
    ///
    /// They are the settlement of the call that created it, the Agent's own cancel, and
    /// otherwise a cancel command added at the Log's end.
    pub fn record_schedules(
        &mut self,
        schedules: &[Schedule],
        sessions: &BTreeMap<SessionId, SessionRecord>,
        blobs: &Path,
    ) -> Result<(), StoreError> {
        let owners: BTreeMap<ScheduleId, SessionId> = schedules
            .iter()
            .map(|schedule| (schedule.id, schedule.session))
            .collect();
        for schedule in schedules {
            let rows = self
                .0
                .get_mut(&schedule.session)
                .map(Vec::as_mut_slice)
                .unwrap_or_default();
            let created = calls(rows, "schedule_create", blobs)?
                .into_iter()
                .find(|settled| settled.result["id"] == schedule.id.to_string())
                .map(|settled| settled.index)
                .ok_or_else(|| {
                    backend(format!(
                        "no schedule_create settlement in Session {} names schedule {}",
                        schedule.session, schedule.id
                    ))
                })?;
            set_change(
                &mut rows[created],
                ScheduleChange::Created {
                    id: schedule.id,
                    rule: schedule.rule.clone(),
                    message: schedule.message.clone(),
                    created_at: schedule.created_at,
                },
            );
        }
        let mut by_agent = BTreeSet::new();
        for (session, rows) in &mut self.0 {
            for Settled {
                index,
                result,
                arguments,
            } in calls(rows, "schedule_cancel", blobs)?
            {
                let id: ScheduleId = serde_json::from_value(arguments["id"].clone())
                    .map_err(|error| backend(format!("schedule_cancel arguments: {error}")))?;
                if result["cancelled"] == true && owners.get(&id) == Some(session) {
                    set_change(&mut rows[index], ScheduleChange::Cancelled { id });
                    by_agent.insert(id);
                }
            }
        }
        for schedule in schedules {
            if schedule.state == ScheduleState::Cancelled && !by_agent.contains(&schedule.id) {
                self.add_cancel(schedule, sessions)?;
            }
        }
        Ok(())
    }

    /// The owner's cancel, or another Session's, left no fact in the Session's Log, and version
    /// 23 did not record when it happened.
    ///
    /// The command follows the Log's last entry, at that entry's time, and takes the schedule's
    /// identity as its own, so upgrading the same database twice writes the same bytes.
    fn add_cancel(
        &mut self,
        schedule: &Schedule,
        sessions: &BTreeMap<SessionId, SessionRecord>,
    ) -> Result<(), StoreError> {
        let owner = sessions
            .get(&schedule.session)
            .ok_or(StoreError::UnknownSession(schedule.session))?;
        let rows = self.0.entry(owner.id).or_default();
        let epoch = owner.binding.epoch;
        let last = rows
            .iter()
            .filter(|row| row.entry.pos.epoch == epoch)
            .map(|row| &row.entry)
            .max_by_key(|entry| entry.pos.seq);
        let (seq, at) = last.map_or((1, owner.created_at), |entry| {
            (entry.pos.seq.0 + 1, entry.at)
        });
        let event = Event {
            id: schedule.id.to_string().parse().map_err(backend)?,
            session: owner.id,
            source: EventSource::Cli,
            body: EventBody::CancelSchedule {
                schedule: schedule.id,
            },
            received_at: at,
        };
        rows.push(Row {
            entry: Entry {
                pos: LogPos {
                    epoch,
                    seq: Seq(seq),
                },
                at,
                body: EntryBody::EventConsumed { event },
            },
            changed: false,
            added: true,
        });
        Ok(())
    }
}

/// A call that settled ok.
struct Settled {
    /// Where its settlement is in the Log.
    index: usize,
    /// What it returned.
    result: Value,
    /// The arguments it accepted.
    arguments: Map<String, Value>,
}

/// The calls of one tool in a Log that settled ok.
///
/// Only an ok settlement holds a result as JSON; a failed one holds its error as text. A result
/// too long to keep in the Log is read in full from its blob in `blobs`.
fn calls(rows: &[Row], tool: &str, blobs: &Path) -> Result<Vec<Settled>, StoreError> {
    let mut started = BTreeSet::new();
    let mut arguments = BTreeMap::new();
    for row in rows {
        match &row.entry.body {
            EntryBody::ToolCallStarted {
                call, capability, ..
            } if capability.name == tool => {
                started.insert(*call);
            }
            EntryBody::AttemptSettled {
                result: AttemptResult::Completed { message, .. },
                ..
            } => {
                for call in message.tool_calls() {
                    arguments.insert(call.id, Arguments::parse(&call.arguments));
                }
            }
            _ => {}
        }
    }
    let mut settled = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let EntryBody::ToolCallSettled {
            call,
            outcome: Settlement::Ok,
            content,
            full,
            ..
        } = &row.entry.body
        else {
            continue;
        };
        if !started.contains(call) {
            continue;
        }
        let full = full
            .map(|hash| content::get_blocking(&blob_path(blobs, &hash), &hash, StoreError::Blob))
            .transpose()?;
        let result = serde_json::from_slice(full.as_deref().unwrap_or(content.as_bytes()))
            .map_err(|error| backend(format!("the result of {tool} call {call}: {error}")))?;
        let Some(Arguments::Object(accepted)) = arguments.remove(call) else {
            return Err(backend(format!(
                "{tool} call {call} settled ok without arguments it accepted"
            )));
        };
        settled.push(Settled {
            index,
            result,
            arguments: accepted,
        });
    }
    Ok(settled)
}

fn set_change(row: &mut Row, change: ScheduleChange) {
    if let EntryBody::ToolCallSettled { schedule, .. } = &mut row.entry.body {
        *schedule = Some(change);
        row.changed = true;
    }
}

/// Record the control plane's facts: each root's creation in the root directory, and the archive
/// of each Session that its parent's archive does not imply.
pub(super) fn control_plane(
    connection: &Connection,
    sessions: &BTreeMap<SessionId, SessionRecord>,
) -> Result<(), StoreError> {
    for row in sessions.values() {
        let inherited = match &row.parent {
            None => {
                sessions::record_root(connection, &creation(row))?;
                None
            }
            Some(parent) => {
                sessions
                    .get(&parent.id)
                    .ok_or(StoreError::UnknownSession(parent.id))?
                    .archived_at
            }
        };
        match (row.archived_at, inherited) {
            (Some(at), inherited) if inherited != Some(at) => {
                connection
                    .execute(
                        "INSERT INTO archives(session_id,at) VALUES (?,?)",
                        params![row.id.to_string(), timestamp(at)],
                    )
                    .map_err(backend)?;
            }
            (None, Some(_)) => {
                return Err(backend(format!(
                    "Session {} is unarchived under an archived parent",
                    row.name
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Every Session and its schedules, folded from the recorded facts by the functions commits use,
/// must equal the rows version 23 kept, in the effective view.
pub(super) fn check(
    connection: &Connection,
    sessions: &BTreeMap<SessionId, SessionRecord>,
    schedules: &[Schedule],
) -> Result<(), StoreError> {
    for row in sessions.values() {
        let log = log::log(connection, row.id, None)?;
        let pending = sessions::pending(connection, row.id)?;
        let mut rebuilt = SessionRecord::fold(
            &recorded_creation(connection, row)?,
            archived_at(connection, row.id)?,
            log.iter().map(|entry| &entry.body),
        );
        let mut folded = ScheduleFold::new(row.id);
        for entry in &log {
            folded.apply(&entry.body);
        }
        if schedules::committed(connection, row.id)? != folded.rows() {
            return Err(backend(format!(
                "the schedules of Session {} differ from the fold of its Log",
                row.name
            )));
        }
        for event in &pending {
            rebuilt.consume(&event.body);
            folded.consume(event);
        }
        if rebuilt != *row {
            return Err(backend(format!(
                "Session {} rebuilt from its facts as {rebuilt:?}, but it was {row:?}",
                row.name
            )));
        }
        let mut kept: Vec<Schedule> = schedules
            .iter()
            .filter(|schedule| schedule.session == row.id)
            .cloned()
            .collect();
        kept.sort_by_key(|schedule| schedule.id);
        if folded.rows() != kept {
            return Err(backend(format!(
                "the schedules of Session {} rebuilt from its facts as {:?}, but they were {kept:?}",
                row.name,
                folded.rows()
            )));
        }
    }
    Ok(())
}

/// A root's creation record from the root directory, a child's from its parent's Log, where
/// exactly one settlement created it.
fn recorded_creation(connection: &Connection, row: &SessionRecord) -> Result<Creation, StoreError> {
    let Some(parent) = &row.parent else {
        return root(connection, row.id)?
            .ok_or_else(|| backend(format!("root {} is not in the root directory", row.name)));
    };
    let log = log::log(connection, parent.id, None)?;
    let mut creations = log
        .iter()
        .filter_map(|entry| entry.body.created_child())
        .filter(|creation| creation.id == row.id);
    match (creations.next(), creations.next()) {
        (Some(creation), None) => Ok(creation.clone()),
        (found, _) => Err(backend(format!(
            "Session {}: {} creation records in its parent's Log, not one",
            row.name,
            if found.is_some() { "several" } else { "no" }
        ))),
    }
}

/// A root's creation record, as the root directory keeps it.
fn root(connection: &Connection, id: SessionId) -> Result<Option<Creation>, StoreError> {
    connection
        .query_row(
            "SELECT name,created_at,node,profile,working_dir_node,working_dir_path
             FROM root_directory WHERE id=?",
            [id.to_string()],
            |row| {
                let working_dir =
                    WorkingDir::new(rows::text(row, 4)?, row.get::<_, String>(5)?.into()).map_err(
                        |error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        },
                    )?;
                Ok(Creation {
                    id,
                    name: rows::text(row, 0)?,
                    parent: None,
                    created_at: rows::text(row, 1)?,
                    node: rows::text(row, 2)?,
                    profile: rows::text(row, 3)?,
                    working_dir,
                })
            },
        )
        .optional()
        .map_err(backend)
}

/// The archive recorded for this Session, else for its nearest recorded ancestor.
fn archived_at(
    connection: &Connection,
    id: SessionId,
) -> Result<Option<DateTime<Utc>>, StoreError> {
    connection
        .query_row(
            "WITH RECURSIVE up(id, parent, depth) AS (
                 SELECT id, parent, 0 FROM sessions WHERE id=?
                 UNION ALL SELECT s.id, s.parent, up.depth+1 FROM sessions s JOIN up ON s.id=up.parent
             )
             SELECT a.at FROM up JOIN archives a ON a.session_id=up.id ORDER BY up.depth LIMIT 1",
            [id.to_string()],
            |row| rows::text(row, 0),
        )
        .optional()
        .map_err(backend)
}
