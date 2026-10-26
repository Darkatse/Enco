//! Accepting input events, each once by its ID, together with the connection state that read
//! them.
use super::{backend, encode, sessions, signed};
use enco_core::*;
use enco_kernel::{Acceptance, Accepted, ConnectionWrite, StoreError};
use rusqlite::{Connection, Transaction, TransactionBehavior, params};

pub(super) fn accept(
    connection: &mut Connection,
    node: NodeId,
    events: &[Event],
    write: Option<&ConnectionWrite>,
) -> Result<Acceptance, StoreError> {
    let events = encode_events(events)?;
    let prepared = write
        .map(|write| {
            let settlement = write
                .settlement
                .as_ref()
                .map(|settlement| Ok::<_, StoreError>((&settlement.delivery, encode(settlement)?)))
                .transpose()?;
            Ok::<_, StoreError>((write, encode(&write.state)?, settlement))
        })
        .transpose()?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let session = write
        .and_then(|write| write.session.as_ref())
        .map(|new| sessions::insert_new(&tx, node, new))
        .transpose()?;
    let events = events_in(&tx, &events)?;
    if let Some((write, state, settlement)) = prepared {
        if let Some(update) = &write.update {
            tx.execute(
                "INSERT INTO connection_updates(connection,update_key) VALUES (?,?)",
                params![write.key, update],
            )
            .map_err(backend)?;
        }
        tx.execute(
            "INSERT INTO connections(key,state) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET state=excluded.state",
            params![write.key, state],
        ).map_err(backend)?;
        if let Some((delivery, body)) = settlement {
            tx.execute(
                "INSERT INTO deliveries(connection,session_id,epoch,seq,body) VALUES (?,?,?,?,?)",
                params![
                    write.key,
                    delivery.session.to_string(),
                    signed(delivery.pos.epoch.0)?,
                    signed(delivery.pos.seq.0)?,
                    body
                ],
            )
            .map_err(backend)?;
        }
    }
    tx.commit().map_err(backend)?;
    Ok(Acceptance { events, session })
}

pub(super) fn encode_events(
    events: &[Event],
) -> Result<Vec<(EventId, SessionId, String)>, StoreError> {
    events
        .iter()
        .map(|event| Ok((event.id, event.session, encode(event)?)))
        .collect()
}

/// Reused by owners that accept inputs together with their own state transition.
pub(super) fn events_in(
    tx: &Transaction<'_>,
    events: &[(EventId, SessionId, String)],
) -> Result<Vec<Accepted>, StoreError> {
    let mut accepted = Vec::with_capacity(events.len());
    for (id, session, body) in events {
        let inserted = tx.execute(
            "INSERT INTO inbox(event_id,session_id,event) SELECT ?1,?2,?3 WHERE NOT EXISTS (SELECT 1 FROM log WHERE event_id=?1) ON CONFLICT(event_id) DO NOTHING",
            params![id.to_string(), session.to_string(), body],
        ).map_err(backend)?;
        accepted.push(if inserted == 0 {
            Accepted::Duplicate
        } else {
            Accepted::New
        });
    }
    Ok(accepted)
}
