//! Accepting input events, each once by its ID, together with the connection state that read
//! them.
use super::{backend, encode, schedules, sessions, signed};
use enco_core::*;
use enco_kernel::{Acceptance, Accepted, ConnectionWrite, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

pub(super) fn accept(
    connection: &mut Connection,
    node: NodeId,
    working_dir: &WorkingDir,
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
        .map(|new| sessions::create_root(&tx, node, working_dir, new))
        .transpose()?;
    let events = events_in(&tx, &events)?;
    if let Some((write, state, settlement)) = prepared {
        tx.execute(
            "INSERT INTO connections(key,state) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET state=excluded.state",
            params![write.key, state],
        ).map_err(backend)?;
        if let Some(update) = &write.update {
            tx.execute(
                "INSERT INTO connection_updates(connection,update_key) VALUES (?,?)",
                params![write.key, update],
            )
            .map_err(backend)?;
        }
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

/// An Event with its stored document, encoded before the transaction that writes it.
pub(super) struct Encoded<'a> {
    pub event: &'a Event,
    pub document: String,
}

pub(super) fn encode_events(events: &[Event]) -> Result<Vec<Encoded<'_>>, StoreError> {
    events
        .iter()
        .map(|event| {
            Ok(Encoded {
                event,
                document: encode(event)?,
            })
        })
        .collect()
}

/// The Inbox's single write function, reused by owners that accept inputs together with their
/// own state transition.
///
/// An Event is written once by its ID and never to an archived Session; one naming a schedule, a
/// reminder or an owner's cancel, only while that schedule is active in the effective view, so
/// no reminder follows an accepted cancel and a schedule is cancelled once.
pub(super) fn events_in(
    tx: &Transaction<'_>,
    events: &[Encoded<'_>],
) -> Result<Vec<Accepted>, StoreError> {
    events
        .iter()
        .map(|Encoded { event, document }| {
            let archived: Option<String> = tx
                .query_row(
                    "SELECT archived_at FROM sessions WHERE id=?",
                    [event.session.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?
                .ok_or(StoreError::UnknownSession(event.session))?;
            if archived.is_some() {
                return Ok(Accepted::Archived);
            }
            let seen: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM inbox WHERE event_id=?1)
                         OR EXISTS(SELECT 1 FROM log WHERE event_id=?1)",
                    [event.id.to_string()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if seen {
                return Ok(Accepted::Duplicate);
            }
            if let Some(schedule) = event.body.schedule()
                && schedules::effective(tx, event.session)?
                    .get(schedule)
                    .is_none_or(|schedule| schedule.state != ScheduleState::Active)
            {
                return Ok(Accepted::Inactive);
            }
            tx.execute(
                "INSERT INTO inbox(event_id,session_id,event) VALUES (?,?,?)",
                params![event.id.to_string(), event.session.to_string(), document],
            )
            .map_err(backend)?;
            Ok(Accepted::New)
        })
        .collect()
}
