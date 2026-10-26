//! Channel connections' committed state and their settled deliveries.
use super::{backend, rows};
use crate::limits::RECENT_DELIVERY_FAILURES;
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn update_seen(
    connection: &Connection,
    key: &str,
    update: &str,
) -> Result<bool, StoreError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM connection_updates WHERE connection=? AND update_key=?)",
            params![key, update],
            |r| r.get(0),
        )
        .map_err(backend)
}

/// A connection's committed state, written together with the input it accepted.
pub(super) fn state(
    connection: &Connection,
    key: &str,
) -> Result<Option<serde_json::Value>, StoreError> {
    connection
        .query_row("SELECT state FROM connections WHERE key=?", [key], |r| {
            rows::document(r, 0)
        })
        .optional()
        .map_err(backend)
}

/// The latest failed or unknown deliveries, newest first, in commit order.
pub(super) fn recent_failures(
    connection: &Connection,
    key: &str,
) -> Result<Vec<DeliverySettlement>, StoreError> {
    let mut query = connection
        .prepare(
            "SELECT body FROM deliveries
             WHERE connection=? AND outcome IN ('failed', 'unknown')
             ORDER BY order_no DESC LIMIT ?",
        )
        .map_err(backend)?;
    query
        .query_map(params![key, RECENT_DELIVERY_FAILURES], |r| {
            rows::document(r, 0)
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)
}

/// The delivery to `target` whose confirmed parts include the protocol message `message`.
pub(super) fn delivered(
    connection: &Connection,
    key: &str,
    target: &str,
    message: &str,
) -> Result<Option<Delivery>, StoreError> {
    let settlement: Option<DeliverySettlement> = connection
        .query_row(
            "SELECT body FROM deliveries
             WHERE connection=?1
               AND json_extract(body,'$.delivery.target')=?2
               AND EXISTS (SELECT 1 FROM json_each(body,'$.messages') WHERE value=?3)",
            params![key, target, message],
            |r| rows::document(r, 0),
        )
        .optional()
        .map_err(backend)?;
    Ok(settlement.map(|settlement| settlement.delivery))
}
