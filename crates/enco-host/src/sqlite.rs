//! Shared SQLite representation rules; each authority retains its own schema and owner.
use enco_core::{DateTime, Utc};
use rusqlite::{Connection, Row, types::Type};
use serde::de::DeserializeOwned;
use std::path::Path;
pub(crate) fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )?;
    Ok(connection)
}

pub(crate) fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn text<T: DeserializeOwned>(row: &Row<'_>, col: usize) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(row.get(col)?))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(col, Type::Text, Box::new(e)))
}

pub(crate) fn document<T: DeserializeOwned>(row: &Row<'_>, col: usize) -> rusqlite::Result<T> {
    let raw: String = row.get(col)?;
    serde_json::from_str(&raw)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(col, Type::Text, Box::new(e)))
}

pub(crate) fn unsigned(row: &Row<'_>, col: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(col)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(col, value))
}
