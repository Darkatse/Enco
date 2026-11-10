//! Opening a version 23 database upgrades it to version 24 in one transaction, after writing a
//! copy of it; no other code reads an older format.
//!
//! The upgrade is checked by rebuilding: every rewritten document decodes as its current type, the
//! schema equals a new database's, every reference resolves, and folding the recorded facts with
//! the functions commits use gives back every Session and schedule row as version 23 kept it.
//! Otherwise nothing commits. Artifact files are the registry's to check when it loads them.
use super::{backend, rows, sessions};
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
mod documents;
mod facts;
mod registry;

/// The only version this code upgrades from.
pub(super) const FROM: u32 = 23;

/// The tables and indexes version 24 adds, removes or defines differently; the migration
/// rewrites their contents, and every other object must be defined as before.
const REPLACED: [&str; 13] = [
    "archives",
    "config",
    "config_generations",
    "generation_health",
    "generations",
    "plugin_generations",
    "plugin_generations_by_plugin",
    "plugins",
    "registration_code",
    "registration_names",
    "replaced_config",
    "replaced_names",
    "root_directory",
];

/// Upgrade the database at `path` to `target`, keeping a copy of it as it was.
///
/// On failure the database is unchanged; the error says why and where the copy is.
/// `blobs` holds the full text of results too long to keep in the Log.
pub(super) fn upgrade(
    connection: &mut Connection,
    path: &Path,
    blobs: &Path,
    target: u32,
) -> Result<(), StoreError> {
    let copy = backup(connection, path)?;
    tracing::info!(copy = %copy.display(), "upgrading the database from version {FROM}");
    // Replacing tables that reference each other needs the checks off; the transaction checks
    // every reference before it commits.
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .map_err(backend)?;
    let upgraded = upgrade_in(connection, blobs, target);
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(backend)?;
    upgraded.map_err(|error| StoreError::Migration {
        from: FROM,
        reason: format!(
            "{error}; the database is unchanged, and a copy of it is at {}",
            copy.display()
        ),
    })
}

/// A complete copy of the database beside it, named for the version and the time.
fn backup(connection: &Connection, path: &Path) -> Result<PathBuf, StoreError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| backend(format!("{} has no UTF-8 file name", path.display())))?;
    let copy = path.with_file_name(format!(
        "{name}.v{FROM}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    let target = copy
        .to_str()
        .ok_or_else(|| backend(format!("{} is not UTF-8", copy.display())))?;
    connection
        .execute("VACUUM INTO ?", [target])
        .map_err(backend)?;
    Ok(copy)
}

fn upgrade_in(connection: &mut Connection, blobs: &Path, target: u32) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let registry = registry::Registry::read(&tx)?;
    let sessions: BTreeMap<SessionId, SessionRecord> = sessions::committed_all(&tx)?
        .into_iter()
        .map(|row| (row.id, row))
        .collect();
    let schedules = tx
        .prepare(
            "SELECT id,session_id,rule,message,created_at,state,last_due,last_event_id
             FROM schedules ORDER BY id",
        )
        .map_err(backend)?
        .query_map([], rows::schedule)
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let rewrite = documents::Rewrite {
        registry: &registry,
        sessions: &sessions,
    };
    let mut records = documents::Records::new(&tx, &registry);
    let mut logs = facts::Logs::read(&tx, &rewrite, &mut records)?;
    logs.record_schedules(&schedules, &sessions, blobs)?;
    logs.write(&tx)?;
    documents::rewrite_tables(&tx, &rewrite, &mut records)?;
    schema(&tx)?;
    registry.write(&tx)?;
    facts::control_plane(&tx, &sessions)?;
    logs.materialize(&tx)?;
    check_references(&tx)?;
    facts::check(&tx, &sessions, &schedules)?;
    tx.pragma_update(None, "user_version", target)
        .map_err(backend)?;
    tx.commit().map_err(backend)
}

/// Every object of a new database, by name: its kind and definition.
fn objects(connection: &Connection) -> Result<BTreeMap<String, (String, String)>, StoreError> {
    connection
        .prepare(
            "SELECT name,type,sql FROM sqlite_master
             WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'",
        )
        .map_err(backend)?
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)
}

/// Replace the objects version 24 defines differently by the definitions in `schema.sql`.
fn schema(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let target = Connection::open_in_memory().map_err(backend)?;
    target
        .execute_batch(include_str!("schema.sql"))
        .map_err(backend)?;
    let wanted = objects(&target)?;
    let present = objects(tx)?;
    let changed: BTreeSet<&str> = wanted
        .keys()
        .chain(present.keys())
        .filter(|name| wanted.get(*name) != present.get(*name))
        .map(String::as_str)
        .collect();
    if changed != BTreeSet::from(REPLACED) {
        return Err(backend(format!(
            "version 24 differs from this database in {changed:?}, not only in {REPLACED:?}"
        )));
    }
    for (name, (kind, _)) in &present {
        if kind == "table" && changed.contains(name.as_str()) {
            tx.execute_batch(&format!("DROP TABLE {name}"))
                .map_err(backend)?;
        }
    }
    // Create in the order `schema.sql` does, so each definition reads as it was written.
    let order = target
        .prepare("SELECT name FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        .map_err(backend)?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    for name in order.iter().filter(|name| changed.contains(name.as_str())) {
        tx.execute_batch(&wanted[name].1).map_err(backend)?;
    }
    if objects(tx)? != wanted {
        return Err(backend("the schema differs from a new database's"));
    }
    Ok(())
}

/// Every reference resolves: the foreign keys hold and the registry reads back.
fn check_references(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let violation: Option<String> = tx
        .prepare("PRAGMA foreign_key_check")
        .map_err(backend)?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .next()
        .transpose()
        .map_err(backend)?;
    if let Some(table) = violation {
        return Err(backend(format!("a reference in {table} does not resolve")));
    }
    super::registry::read(tx)?;
    Ok(())
}
