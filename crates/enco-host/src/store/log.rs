//! Reading a Session's Log, appended only by its commits, in structural order.
use super::{backend, rows, signed};
use enco_core::*;
use enco_kernel::{ConsumedMessage, StoreError};
use rusqlite::{Connection, OptionalExtension, params};

/// The entries after `after`, or the whole Log.
pub(super) fn log(
    connection: &Connection,
    session: SessionId,
    after: Option<LogPos>,
) -> Result<Vec<Entry>, StoreError> {
    let position = after.unwrap_or(LogPos {
        epoch: Epoch(0),
        seq: Seq(0),
    });
    query_entries(
        connection,
        "SELECT epoch,seq,at,body
         FROM log
         WHERE session_id=? AND (epoch,seq)>(?,?)
         ORDER BY epoch,seq",
        params![
            session.to_string(),
            signed(position.epoch.0)?,
            signed(position.seq.0)?
        ],
    )
}

pub(super) fn log_before(
    connection: &Connection,
    session: SessionId,
    before: Option<LogPos>,
    limit: usize,
) -> Result<Vec<Entry>, StoreError> {
    let limit = i64::try_from(limit).map_err(backend)?;
    // The bound is its own statement: behind `? IS NULL OR`, SQLite could not seek to it in the
    // key and would scan from the end of the Session's Log on every page.
    let mut entries = match before {
        None => query_entries(
            connection,
            "SELECT epoch,seq,at,body
             FROM log
             WHERE session_id=?
             ORDER BY epoch DESC, seq DESC
             LIMIT ?",
            params![session.to_string(), limit],
        ),
        Some(before) => query_entries(
            connection,
            "SELECT epoch,seq,at,body
             FROM log
             WHERE session_id=? AND (epoch,seq)<(?,?)
             ORDER BY epoch DESC, seq DESC
             LIMIT ?",
            params![
                session.to_string(),
                signed(before.epoch.0)?,
                signed(before.seq.0)?,
                limit
            ],
        ),
    }?;
    entries.reverse();
    Ok(entries)
}

pub(super) fn log_entry(
    connection: &Connection,
    session: SessionId,
    pos: LogPos,
) -> Result<Option<Entry>, StoreError> {
    // SQLite stores signed integers, so no Log holds a position beyond them.
    let (Ok(epoch), Ok(seq)) = (i64::try_from(pos.epoch.0), i64::try_from(pos.seq.0)) else {
        return Ok(None);
    };
    connection
        .query_row(
            "SELECT epoch,seq,at,body
             FROM log
             WHERE session_id=? AND epoch=? AND seq=?",
            params![session.to_string(), epoch, seq],
            rows::entry,
        )
        .optional()
        .map_err(backend)
}

fn query_entries(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Entry>, StoreError> {
    connection
        .prepare(sql)
        .map_err(backend)?
        .query_map(params, rows::entry)
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
