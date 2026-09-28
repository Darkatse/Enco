use super::{backend, encode, rows};
use enco_core::*;
use enco_kernel::{Commit, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashSet;

pub(super) fn commit(
    connection: &mut Connection,
    node: NodeId,
    session: SessionId,
    commit: Commit,
) -> Result<(), StoreError> {
    let first = commit.entries.first().ok_or(StoreError::EmptyCommit)?;
    // Ownership, append positions and Inbox consumption are checked in the same transaction.
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
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
    for (i, entry) in commit.entries.iter().enumerate() {
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
    consume_events(&tx, session, &commit)?;

    for entry in &commit.entries {
        tx.execute(
            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,?,?,?,?)",
            params![
                session.to_string(),
                super::signed(entry.pos.epoch.0)?,
                super::signed(entry.pos.seq.0)?,
                super::timestamp(entry.at),
                encode(&entry.body)?
            ],
        )
        .map_err(backend)?;
    }
    tx.commit().map_err(backend)
}

fn consume_events(
    tx: &Transaction<'_>,
    session: SessionId,
    commit: &Commit,
) -> Result<(), StoreError> {
    let accepted: Vec<_> = commit
        .entries
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::EventConsumed { event } => Some((event, e.pos)),
            _ => None,
        })
        .collect();
    let consumed: HashSet<_> = commit.consumed.iter().copied().collect();
    if consumed.len() != commit.consumed.len()
        || consumed.len() != accepted.len()
        || accepted
            .iter()
            .any(|(e, _)| !consumed.contains(&e.id) || e.session != session)
    {
        return Err(StoreError::Inbox(
            "EventConsumed and consumed IDs must correspond exactly".into(),
        ));
    }
    for (event, pos) in accepted {
        let stored: Option<Event> = tx
            .query_row(
                "SELECT event
                 FROM inbox
                 WHERE event_id=? AND session_id=? AND consumed_seq IS NULL",
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
        tx.execute(
            "UPDATE inbox SET consumed_epoch=?,consumed_seq=? WHERE event_id=?",
            params![
                super::signed(pos.epoch.0)?,
                super::signed(pos.seq.0)?,
                event.id.to_string()
            ],
        )
        .map_err(backend)?;
    }
    Ok(())
}
