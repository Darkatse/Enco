//! Appending a Session's Log facts and consuming the input they answer, in one transaction.
use super::{backend, encode, plans::EncodedWrite, rows};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{Commit, CommitOutcome, StoreError};
mod deliveries;
use deliveries::Delivered;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

pub(super) fn commit(
    connection: &mut Connection,
    node: NodeId,
    default_working_dir: &WorkingDir,
    session: SessionId,
    commit: Commit,
    plan: Option<EncodedWrite>,
) -> Result<CommitOutcome, StoreError> {
    let first = commit.entries.first().ok_or(StoreError::EmptyCommit)?;
    let bodies = commit
        .entries
        .iter()
        .map(|entry| encode(&entry.body))
        .collect::<Result<Vec<_>, _>>()?;
    let events = commit
        .deliveries
        .iter()
        .map(|delivery| super::accept::encode_events(std::slice::from_ref(&delivery.event)))
        .collect::<Result<Vec<_>, _>>()?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    check_append(&tx, node, session, first, &commit.entries)?;
    consume_events(&tx, session, &commit)?;

    let halted: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM inbox WHERE session_id=? AND json_extract(event, '$.body.kind')='halt')",
        [session.to_string()], |row| row.get(0),
    ).map_err(backend)?;
    let mut delivered = Vec::new();
    for (delivery, encoded) in commit.deliveries.iter().zip(&events) {
        match deliveries::deliver(&tx, node, default_working_dir, halted, delivery, encoded)? {
            Delivered::Written => delivered.push(delivery.event.session),
            Delivered::Skipped => {}
            Delivered::Rejected(reason) => return Ok(CommitOutcome::Rejected(reason)),
        }
    }
    if let Some(plan) = plan {
        plan.insert(&tx)?;
    }

    for (entry, body) in commit.entries.iter().zip(bodies) {
        tx.execute(
            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,?,?,?,?)",
            params![
                session.to_string(),
                super::signed(entry.pos.epoch.0)?,
                super::signed(entry.pos.seq.0)?,
                timestamp(entry.at),
                body
            ],
        )
        .map_err(backend)?;
    }
    tx.commit().map_err(backend)?;
    Ok(CommitOutcome::Committed { delivered })
}

fn check_append(
    tx: &Transaction<'_>,
    node: NodeId,
    session: SessionId,
    first: &Entry,
    entries: &[Entry],
) -> Result<(), StoreError> {
    let binding = tx
        .query_row(
            "SELECT binding_node,binding_epoch FROM sessions WHERE id=?",
            [session.to_string()],
            |row| {
                Ok(Binding {
                    node: rows::text(row, 0)?,
                    epoch: Epoch(rows::unsigned(row, 1)?),
                })
            },
        )
        .optional()
        .map_err(backend)?
        .ok_or(StoreError::UnknownSession(session))?;
    if binding.node != node || binding.epoch != first.pos.epoch {
        return Err(StoreError::Fenced {
            session,
            binding,
            epoch: first.pos.epoch,
        });
    }
    let next_seq: u64 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq),0)+1 FROM log WHERE session_id=? AND epoch=?",
            params![session.to_string(), super::signed(binding.epoch.0)?],
            |row| rows::unsigned(row, 0),
        )
        .map_err(backend)?;
    for (i, entry) in entries.iter().enumerate() {
        let expected = LogPos {
            epoch: binding.epoch,
            seq: Seq(next_seq + i as u64),
        };
        if entry.pos != expected {
            return Err(StoreError::OutOfOrder {
                session,
                expected,
                got: entry.pos,
            });
        }
    }
    Ok(())
}

fn consume_events(
    tx: &Transaction<'_>,
    session: SessionId,
    commit: &Commit,
) -> Result<(), StoreError> {
    let accepted = commit.entries.iter().filter_map(|e| match &e.body {
        EntryBody::EventConsumed { event } => Some(event),
        _ => None,
    });
    for event in accepted {
        let stored: Option<Event> = tx
            .query_row(
                "SELECT event
                 FROM inbox
                 WHERE event_id=? AND session_id=?",
                params![event.id.to_string(), session.to_string()],
                |row| rows::document(row, 0),
            )
            .optional()
            .map_err(backend)?;
        if stored.as_ref() != Some(event) {
            return Err(StoreError::Inbox(format!(
                "event {} is missing, already consumed or changed",
                event.id
            )));
        }
        tx.execute("DELETE FROM inbox WHERE event_id=?", [event.id.to_string()])
            .map_err(backend)?;
    }
    Ok(())
}
