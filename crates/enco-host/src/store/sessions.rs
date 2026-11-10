//! Session rows, each the fold of its creation record, its archive and its own Log.
//!
//! The Round reads the committed view; every other reader reads the effective one, with the
//! pending commands applied, and the registration's admission every view in between.
use super::{backend, rows, signed};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{NewSession, SessionViews, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

const COLUMNS: &str = "s.id,s.name,s.created_at,s.binding_node,s.binding_epoch,s.profile,
    s.working_dir_node,s.working_dir_path,p.id,p.name,s.archived_at";
const FROM: &str = "sessions s LEFT JOIN sessions p ON p.id = s.parent";

/// Every Session in its effective view.
pub(super) fn all(connection: &Connection) -> Result<Vec<SessionRecord>, StoreError> {
    effective_all(connection, committed_all(connection)?)
}

/// Every unarchived Session with the views ahead of its committed one.
pub(super) fn views(connection: &Connection) -> Result<Vec<SessionViews>, StoreError> {
    select(
        connection,
        &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.archived_at IS NULL ORDER BY s.rowid"),
        [],
    )?
    .into_iter()
    .map(|committed| {
        Ok(SessionViews {
            ahead: ahead(connection, &committed)?,
            committed,
        })
    })
    .collect()
}

/// A Session's committed row and its ancestors', nearest first.
pub(super) fn lineage(
    connection: &Connection,
    id: SessionId,
) -> Result<(SessionRecord, Vec<SessionRecord>), StoreError> {
    let row = |id| committed(connection, id)?.ok_or(StoreError::UnknownSession(id));
    let session = row(id)?;
    let mut ancestors = Vec::new();
    let mut parent = session.parent.as_ref().map(|parent| parent.id);
    while let Some(id) = parent {
        let ancestor = row(id)?;
        parent = ancestor.parent.as_ref().map(|parent| parent.id);
        ancestors.push(ancestor);
    }
    Ok((session, ancestors))
}

/// Every Session's materialized row, in creation order.
pub(super) fn committed_all(connection: &Connection) -> Result<Vec<SessionRecord>, StoreError> {
    select(
        connection,
        &format!("SELECT {COLUMNS} FROM {FROM} ORDER BY s.rowid"),
        [],
    )
}

pub(super) fn children(
    connection: &Connection,
    parent: SessionId,
) -> Result<Vec<SessionRecord>, StoreError> {
    let rows = select(
        connection,
        &format!(
            "SELECT {COLUMNS} FROM {FROM} WHERE s.parent=? AND s.archived_at IS NULL ORDER BY s.rowid"
        ),
        [parent.to_string()],
    )?;
    effective_all(connection, rows)
}

/// The unarchived Session with this name, in its effective view.
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
        .map_err(backend)?
        .map(|row| effective(connection, row))
        .transpose()
}

/// A Session's effective view: its committed row with its pending commands applied.
pub(super) fn get(
    connection: &Connection,
    id: SessionId,
) -> Result<Option<SessionRecord>, StoreError> {
    committed(connection, id)?
        .map(|row| effective(connection, row))
        .transpose()
}

/// A Session's committed view: the materialized row.
pub(super) fn committed(
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

fn select(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<SessionRecord>, StoreError> {
    connection
        .prepare(sql)
        .map_err(backend)?
        .query_map(params, rows::session)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

/// The same fold that commits a consumed command, continued over the pending Inbox in order.
fn effective(connection: &Connection, mut row: SessionRecord) -> Result<SessionRecord, StoreError> {
    for event in pending(connection, row.id)? {
        row.consume(&event.body);
    }
    Ok(row)
}

/// The same fold as `effective`, keeping the row after each pending Event: every committed view
/// the Session may come to read.
fn ahead(
    connection: &Connection,
    committed: &SessionRecord,
) -> Result<Vec<SessionRecord>, StoreError> {
    let mut row = committed.clone();
    Ok(pending(connection, row.id)?
        .iter()
        .map(|event| {
            row.consume(&event.body);
            row.clone()
        })
        .collect())
}

fn effective_all(
    connection: &Connection,
    rows: Vec<SessionRecord>,
) -> Result<Vec<SessionRecord>, StoreError> {
    rows.into_iter()
        .map(|row| effective(connection, row))
        .collect()
}

/// Write a row folded from a creation record.
fn materialize(tx: &Transaction<'_>, row: &SessionRecord) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO sessions(id,name,created_at,binding_node,binding_epoch,profile,working_dir_node,working_dir_path,parent)
         VALUES (?,?,?,?,?,?,?,?,?)",
        params![
            row.id.to_string(),
            row.name.as_str(),
            timestamp(row.created_at),
            row.binding.node.to_string(),
            signed(row.binding.epoch.0)?,
            row.profile.as_str(),
            row.working_dir.node.to_string(),
            row.working_dir.path_str(),
            row.parent.as_ref().map(|parent| parent.id.to_string()),
        ],
    )
    .map_err(backend)?;
    Ok(())
}

/// Fold facts the Session's commit appends into its row.
pub(super) fn apply(
    tx: &Transaction<'_>,
    session: SessionId,
    entries: &[Entry],
) -> Result<(), StoreError> {
    let before = committed(tx, session)?.ok_or(StoreError::UnknownSession(session))?;
    let mut row = before.clone();
    for entry in entries {
        row.apply(&entry.body);
    }
    if row != before {
        tx.execute(
            "UPDATE sessions SET profile=?,working_dir_node=?,working_dir_path=? WHERE id=?",
            params![
                row.profile.as_str(),
                row.working_dir.node.to_string(),
                row.working_dir.path_str(),
                session.to_string()
            ],
        )
        .map_err(backend)?;
    }
    Ok(())
}

/// Create a root through the root directory if no unarchived Session has the name.
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
    if named(&tx, name)?.is_none() {
        create_root(
            &tx,
            node,
            working_dir,
            &NewSession {
                id: SessionId::new(),
                name: name.clone(),
                profile: DEFAULT_PROFILE.parse().map_err(backend)?,
                created_at,
            },
        )?;
    }
    let result = named(&tx, name)?.ok_or_else(|| backend("ensured Session is missing"))?;
    tx.commit().map_err(backend)?;
    Ok(result)
}

/// The root directory records a root under a free name, suffixed when occupied, and the row
/// starts from that record.
pub(super) fn create_root(
    tx: &Transaction<'_>,
    node: NodeId,
    working_dir: &WorkingDir,
    new: &NewSession,
) -> Result<SessionRecord, StoreError> {
    let creation = Creation {
        id: new.id,
        name: available_root(tx, &new.name)?,
        parent: None,
        created_at: new.created_at,
        node,
        profile: new.profile.clone(),
        working_dir: working_dir.clone(),
    };
    record_root(tx, &creation)?;
    let row = SessionRecord::created(&creation);
    materialize(tx, &row)?;
    Ok(row)
}

/// Write a root's creation record in the root directory.
pub(super) fn record_root(connection: &Connection, creation: &Creation) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO root_directory(id,name,created_at,node,profile,working_dir_node,working_dir_path)
             VALUES (?,?,?,?,?,?,?)",
            params![
                creation.id.to_string(),
                creation.name.as_str(),
                timestamp(creation.created_at),
                creation.node.to_string(),
                creation.profile.as_str(),
                creation.working_dir.node.to_string(),
                creation.working_dir.path_str(),
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// A child's row, from the creation record its parent's commit appends in the same transaction.
///
/// The parent must have no halt waiting. It is then unarchived and its actor alone names its
/// children, in call order, so the name is free; the unique index on unarchived names holds it.
pub(super) fn create_child(tx: &Transaction<'_>, creation: &Creation) -> Result<(), StoreError> {
    materialize(tx, &SessionRecord::created(creation))
}

fn available_root(tx: &Transaction<'_>, name: &SessionName) -> Result<SessionName, StoreError> {
    let occupied = tx
        .prepare(
        "SELECT name FROM sessions WHERE archived_at IS NULL AND (name=?1 OR substr(name,1,length(?1)+1)=?1||'-')"
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
        candidate = SessionName::root(&format!("{name}-{suffix}")).map_err(backend)?;
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
        // One control-plane fact archives the whole subtree; the rows follow it at once, so the
        // names are free as soon as it commits.
        tx.execute(
            "INSERT INTO archives(session_id,at) VALUES (?,?)",
            params![root.to_string(), timestamp(at)],
        )
        .map_err(backend)?;
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
