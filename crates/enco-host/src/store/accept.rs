use super::{backend, encode, signed};
use enco_core::*;
use enco_kernel::{Accepted, ConnectionWrite, StoreError};
use rusqlite::{Connection, Transaction, TransactionBehavior, params};

pub(super) fn accept(
    connection: &mut Connection,
    events: &[Event],
    write: Option<&ConnectionWrite>,
) -> Result<Vec<Accepted>, StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let accepted = events_in(&tx, events)?;
    if let Some(write) = write {
        tx.execute(
            "INSERT INTO connections(key,state) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET state=excluded.state",
            params![write.key, encode(&write.state)?],
        ).map_err(backend)?;
        if let Some(settlement) = &write.settlement {
            let delivery = &settlement.delivery;
            tx.execute(
                "INSERT INTO deliveries(connection,session_id,epoch,seq,body) VALUES (?,?,?,?,?)",
                params![
                    write.key,
                    delivery.session.to_string(),
                    signed(delivery.pos.epoch.0)?,
                    signed(delivery.pos.seq.0)?,
                    encode(settlement)?
                ],
            )
            .map_err(backend)?;
        }
    }
    tx.commit().map_err(backend)?;
    Ok(accepted)
}

/// Reused by owners that accept inputs together with their own state transition.
pub(super) fn events_in(
    tx: &Transaction<'_>,
    events: &[Event],
) -> Result<Vec<Accepted>, StoreError> {
    let mut accepted = Vec::with_capacity(events.len());
    for event in events {
        let inserted = tx.execute(
            "INSERT INTO inbox(event_id,session_id,event) VALUES (?,?,?) ON CONFLICT(event_id) DO NOTHING",
            params![event.id.to_string(), event.session.to_string(), encode(event)?],
        ).map_err(backend)?;
        accepted.push(if inserted == 0 {
            Accepted::Duplicate
        } else {
            Accepted::New
        });
    }
    Ok(accepted)
}
