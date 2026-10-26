//! Session records: their names, ownership bindings and selected profiles.
use super::{backend, rows, signed};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{ConsumedMessage, NewSession, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

const COLUMNS: &str = "id,name,created_at,binding_node,binding_epoch,profile";

pub(super) fn all(connection: &Connection) -> Result<Vec<SessionRecord>, StoreError> {
    let mut query = connection
        .prepare(&format!("SELECT {COLUMNS} FROM sessions ORDER BY name"))
        .map_err(backend)?;
    query
        .query_map([], rows::session)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

pub(super) fn named(
    connection: &Connection,
    name: &str,
) -> Result<Option<SessionRecord>, StoreError> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM sessions WHERE name=?"),
            [name],
            rows::session,
        )
        .optional()
        .map_err(backend)
}

pub(super) fn get(
    connection: &Connection,
    id: SessionId,
) -> Result<Option<SessionRecord>, StoreError> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM sessions WHERE id=?"),
            [id.to_string()],
            rows::session,
        )
        .optional()
        .map_err(backend)
}

pub(super) fn set_profile(
    connection: &Connection,
    id: SessionId,
    profile: &str,
) -> Result<(), StoreError> {
    let changed = connection
        .execute(
            "UPDATE sessions SET profile=? WHERE id=?",
            params![profile, id.to_string()],
        )
        .map_err(backend)?;
    if changed == 0 {
        return Err(StoreError::UnknownSession(id));
    }
    Ok(())
}

/// A new Session starts on this node at the first epoch with the default profile.
pub(super) fn ensure(
    connection: &mut Connection,
    node: NodeId,
    name: &str,
    created_at: DateTime<Utc>,
) -> Result<SessionRecord, StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    tx.execute(
        "INSERT OR IGNORE INTO sessions VALUES (?,?,?,?,?,?)",
        params![
            SessionId::new().to_string(),
            name,
            timestamp(created_at),
            node.to_string(),
            1,
            DEFAULT_PROFILE
        ],
    )
    .map_err(backend)?;
    let result = tx
        .query_row(
            &format!("SELECT {COLUMNS} FROM sessions WHERE name=?"),
            [name],
            rows::session,
        )
        .map_err(backend)?;
    tx.commit().map_err(backend)?;
    Ok(result)
}

/// Insert a Session chosen by its creator, never adopting one with the same display name.
pub(super) fn insert_new(
    tx: &Transaction<'_>,
    node: NodeId,
    new: &NewSession,
) -> Result<SessionRecord, StoreError> {
    let NewSession {
        id,
        name,
        profile,
        created_at,
    } = new;
    let occupied = tx
        .prepare("SELECT name FROM sessions WHERE name=?1 OR substr(name,1,length(?1)+1)=?1||':'")
        .map_err(backend)?
        .query_map([name], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(backend)?;
    let mut candidate = name.clone();
    let mut suffix = 1_u64;
    while occupied.contains(&candidate) {
        suffix += 1;
        candidate = format!("{name}:{suffix}");
    }
    tx.query_row(
        &format!("INSERT INTO sessions(id,name,created_at,binding_node,binding_epoch,profile) VALUES (?,?,?,?,?,?) RETURNING {COLUMNS}"),
        params![id.to_string(), candidate, timestamp(*created_at), node.to_string(), 1, profile],
        rows::session,
    ).map_err(backend)
}

pub(super) fn log(
    connection: &Connection,
    session: SessionId,
    after: Option<LogPos>,
) -> Result<Vec<Entry>, StoreError> {
    let position = after.unwrap_or(LogPos {
        epoch: Epoch(0),
        seq: Seq(0),
    });
    let mut query = connection
        .prepare(
            "SELECT epoch,seq,at,body
             FROM log
             WHERE session_id=? AND (epoch,seq)>(?,?)
             ORDER BY epoch,seq",
        )
        .map_err(backend)?;
    query
        .query_map(
            params![
                session.to_string(),
                signed(position.epoch.0)?,
                signed(position.seq.0)?
            ],
            rows::entry,
        )
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

/// Where the message with this protocol ID, from this channel conversation, was consumed.
///
/// Protocols keep message IDs unique within a conversation, so at most one input matches.
pub(super) fn consumed_message(
    connection: &Connection,
    protocol: &str,
    account: &str,
    conversation: &str,
    message: &str,
) -> Result<Option<ConsumedMessage>, StoreError> {
    // Only an owner's command asks, so this scans the Log instead of indexing every input.
    connection
        .query_row(
            "SELECT session_id,epoch,seq,json_extract(body,'$.event.source.owner')
             FROM log
             WHERE kind='event_consumed'
               AND json_extract(body,'$.event.body.kind')='user_message'
               AND json_extract(body,'$.event.source.protocol')=?
               AND json_extract(body,'$.event.source.account')=?
               AND json_extract(body,'$.event.source.conversation')=?
               AND json_extract(body,'$.event.source.message')=?",
            params![protocol, account, conversation, message],
            |r| {
                Ok(ConsumedMessage {
                    session: rows::text(r, 0)?,
                    pos: LogPos {
                        epoch: Epoch(rows::unsigned(r, 1)?),
                        seq: Seq(rows::unsigned(r, 2)?),
                    },
                    owner: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(backend)
}

/// Unconsumed Inbox rows in delivery order; only a commit consumes them.
pub(super) fn pending(
    connection: &Connection,
    session: SessionId,
) -> Result<Vec<Event>, StoreError> {
    let mut query = connection
        .prepare(
            "SELECT event
             FROM inbox
             WHERE session_id=?
             ORDER BY order_no",
        )
        .map_err(backend)?;
    query
        .query_map([session.to_string()], |r| rows::document(r, 0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}
