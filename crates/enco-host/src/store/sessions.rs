//! Session records: their names, ownership bindings and selected profiles.
use super::{backend, rows, signed};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{ConsumedMessage, NewSession, NewSessionOrigin, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

const COLUMNS: &str = "s.id,s.name,s.created_at,s.binding_node,s.binding_epoch,s.profile,
    s.working_dir_node,s.working_dir_path,p.id,p.name,s.archived_at";
const FROM: &str = "sessions s LEFT JOIN sessions p ON p.id = s.parent";

pub(super) fn all(connection: &Connection) -> Result<Vec<SessionRecord>, StoreError> {
    let mut query = connection
        .prepare(&format!("SELECT {COLUMNS} FROM {FROM} ORDER BY s.rowid"))
        .map_err(backend)?;
    query
        .query_map([], rows::session)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

pub(super) fn children(
    connection: &Connection,
    parent: SessionId,
) -> Result<Vec<SessionRecord>, StoreError> {
    let mut query = connection
        .prepare(&format!(
            "SELECT {COLUMNS} FROM {FROM} WHERE s.parent=? AND s.archived_at IS NULL ORDER BY s.rowid"
        ))
        .map_err(backend)?;
    query
        .query_map([parent.to_string()], rows::session)
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)
}

pub(super) fn named(
    connection: &Connection,
    name: &str,
) -> Result<Option<SessionRecord>, StoreError> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.name=? AND s.archived_at IS NULL"),
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
            &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.id=?"),
            [id.to_string()],
            rows::session,
        )
        .optional()
        .map_err(backend)
}

pub(super) fn set_profile(
    connection: &Connection,
    id: SessionId,
    profile: &ProfileName,
) -> Result<(), StoreError> {
    let changed = connection
        .execute(
            "UPDATE sessions SET profile=? WHERE id=?",
            params![profile.as_str(), id.to_string()],
        )
        .map_err(backend)?;
    if changed == 0 {
        return Err(StoreError::UnknownSession(id));
    }
    Ok(())
}

pub(super) fn set_working_dir(
    connection: &Connection,
    id: SessionId,
    working_dir: &WorkingDir,
) -> Result<(), StoreError> {
    let changed = connection
        .execute(
            "UPDATE sessions SET working_dir_node=?,working_dir_path=? WHERE id=?",
            params![
                working_dir.node.to_string(),
                working_dir.path_str(),
                id.to_string()
            ],
        )
        .map_err(backend)?;
    if changed == 0 {
        return Err(StoreError::UnknownSession(id));
    }
    Ok(())
}

/// Create an unarchived root if absent, using the node's default profile and directory.
pub(super) fn ensure(
    connection: &mut Connection,
    node: NodeId,
    working_dir: &WorkingDir,
    name: &SessionName,
    created_at: DateTime<Utc>,
) -> Result<SessionRecord, StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    tx.execute(
        "INSERT INTO sessions(id,name,created_at,binding_node,binding_epoch,profile,working_dir_node,working_dir_path)
         SELECT ?,?,?,?,?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM sessions WHERE name=? AND archived_at IS NULL)",
        params![
            SessionId::new().to_string(),
            name.as_str(),
            timestamp(created_at),
            node.to_string(),
            1,
            DEFAULT_PROFILE,
            working_dir.node.to_string(),
            working_dir.path_str(),
            name.as_str(),
        ],
    ).map_err(backend)?;
    let result = named(&tx, name)?.ok_or_else(|| backend("ensured Session is missing"))?;
    tx.commit().map_err(backend)?;
    Ok(result)
}

/// Insert a creator-owned identity; roots disambiguate active names and children retain theirs.
pub(super) fn insert_new(
    tx: &Transaction<'_>,
    node: NodeId,
    default_working_dir: &WorkingDir,
    new: &NewSession,
) -> Result<SessionRecord, StoreError> {
    let (name, parent, working_dir) = match &new.origin {
        NewSessionOrigin::Root => (available_root(tx, &new.name)?, None, default_working_dir),
        NewSessionOrigin::Child {
            parent,
            working_dir,
        } => (new.name.clone(), Some(*parent), working_dir),
    };
    tx.execute(
        "INSERT INTO sessions(id,name,created_at,binding_node,binding_epoch,profile,working_dir_node,working_dir_path,parent)
         VALUES (?,?,?,?,?,?,?,?,?)",
        params![
            new.id.to_string(),
            name.as_str(),
            timestamp(new.created_at),
            node.to_string(),
            1,
            new.profile.as_str(),
            working_dir.node.to_string(),
            working_dir.path_str(),
            parent.map(|id| id.to_string()),
        ],
    ).map_err(backend)?;
    get(tx, new.id)?.ok_or(StoreError::UnknownSession(new.id))
}

fn available_root(tx: &Transaction<'_>, name: &SessionName) -> Result<SessionName, StoreError> {
    let occupied = tx
        .prepare(
        "SELECT name FROM sessions WHERE archived_at IS NULL AND (name=?1 OR substr(name,1,length(?1)+1)=?1||':')"
        )
        .map_err(backend)?
        .query_map([name.as_str()], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(backend)?;
    let mut candidate = name.clone();
    let mut suffix = 1_u64;
    while occupied.contains(candidate.as_str()) {
        suffix += 1;
        candidate = SessionName::root(&format!("{name}:{suffix}")).map_err(backend)?;
    }
    Ok(candidate)
}

/// Accept a halt for every active descendant and optionally archive them in the same commit;
/// returns them root first.
pub(super) fn halt_subtree(
    connection: &mut Connection,
    root: SessionId,
    source: EventSource,
    halt: Halt,
    at: DateTime<Utc>,
    archive: bool,
) -> Result<Vec<SessionRef>, StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let session = get(&tx, root)?.ok_or(StoreError::UnknownSession(root))?;
    if session.archived_at.is_some() {
        return Ok(Vec::new());
    }
    let sessions = tx
        .prepare(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM sessions WHERE id=? AND archived_at IS NULL
            UNION ALL SELECT s.id FROM sessions s JOIN subtree p ON s.parent=p.id WHERE s.archived_at IS NULL
         ) SELECT s.id, s.name FROM subtree JOIN sessions s USING (id)"
        )
        .map_err(backend)?
        .query_map([root.to_string()], |row| {
            Ok(SessionRef {
                id: rows::text(row, 0)?,
                name: rows::text(row, 1)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let events = sessions
        .iter()
        .map(|session| Event {
            id: EventId::new(),
            session: session.id,
            source: if session.id == root {
                source.clone()
            } else {
                EventSource::Parent
            },
            body: EventBody::Halt(halt.clone()),
            received_at: at,
        })
        .collect::<Vec<_>>();
    super::accept::events_in(&tx, &super::accept::encode_events(&events)?)?;
    if archive {
        for session in &sessions {
            tx.execute(
                "UPDATE sessions SET archived_at=? WHERE id=?",
                params![timestamp(at), session.id.to_string()],
            )
            .map_err(backend)?;
        }
    }
    tx.commit().map_err(backend)?;
    Ok(sessions)
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

/// Stop reading the Log as soon as the fold has every fact; the newest entries decide them.
pub(super) fn child_state(
    connection: &Connection,
    child: SessionId,
) -> Result<ChildState, StoreError> {
    let mut fold = ChildStateFold::default();
    for event in pending(connection, child)?.iter().rev() {
        if fold.input(event).is_break() {
            return Ok(fold.finish());
        }
    }
    let mut query = connection
        .prepare("SELECT body FROM log WHERE session_id=? ORDER BY epoch DESC, seq DESC")
        .map_err(backend)?;
    let mut entries = query.query([child.to_string()]).map_err(backend)?;
    while let Some(row) = entries.next().map_err(backend)? {
        let body: EntryBody = rows::document(row, 0).map_err(backend)?;
        if fold.entry(&body).is_break() {
            break;
        }
    }
    Ok(fold.finish())
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
