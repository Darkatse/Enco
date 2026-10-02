use super::{backend, encode, rows, signed};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{NewGeneration, RegistryState, StoreError};
use rusqlite::{Connection, TransactionBehavior, params};

pub(super) fn read(connection: &Connection) -> Result<RegistryState, StoreError> {
    let mut generations = connection.prepare("SELECT id,plugin_id,artifact,config,origin,status,created_at FROM generations ORDER BY id").map_err(backend)?;
    let generations = generations
        .query_map([], |row| {
            Ok(GenerationRecord {
                id: GenerationId(rows::unsigned(row, 0)?),
                plugin: rows::text(row, 1)?,
                artifact: rows::text(row, 2)?,
                config: rows::document(row, 3)?,
                origin: rows::text(row, 4)?,
                status: rows::text(row, 5)?,
                created_at: rows::text(row, 6)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let mut active = connection
        .prepare("SELECT id,active FROM plugins ORDER BY id")
        .map_err(backend)?;
    let active = active
        .query_map([], |row| {
            let id: Option<i64> = row.get(1)?;
            Ok((
                rows::text(row, 0)?,
                id.map(|_| rows::unsigned(row, 1).map(GenerationId))
                    .transpose()?,
            ))
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    Ok(RegistryState {
        generations,
        active,
    })
}

pub(super) fn insert(
    connection: &mut Connection,
    generation: &NewGeneration,
    activate: bool,
) -> Result<GenerationId, StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let plugin = generation.plugin.to_string();
    tx.execute(
        "INSERT INTO plugins(id,active) VALUES (?,NULL) ON CONFLICT(id) DO NOTHING",
        [&plugin],
    )
    .map_err(backend)?;
    let origin = match generation.origin {
        Origin::Factory => "factory",
        Origin::Deployed => "deployed",
    };
    let status = match generation.status {
        GenerationStatus::Healthy => "healthy",
        GenerationStatus::Failed => "failed",
    };
    tx.execute("INSERT INTO generations(plugin_id,artifact,config,origin,status,created_at) VALUES (?,?,?,?,?,?)", params![plugin, generation.artifact.to_string(), encode(&generation.config)?, origin, status, timestamp(generation.created_at)]).map_err(backend)?;
    let id = tx.last_insert_rowid();
    if activate {
        tx.execute(
            "UPDATE plugins SET active=? WHERE id=?",
            params![id, plugin],
        )
        .map_err(backend)?;
    }
    tx.commit().map_err(backend)?;
    Ok(GenerationId(u64::try_from(id).map_err(backend)?))
}

pub(super) fn activate(
    connection: &mut Connection,
    plugin: PluginId,
    to: Option<GenerationId>,
    failed: &[GenerationId],
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    for id in failed {
        let changed = tx
            .execute(
                "UPDATE generations SET status='failed' WHERE id=? AND plugin_id=?",
                params![signed(id.0)?, plugin.to_string()],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(StoreError::UnknownGeneration(*id));
        }
    }
    let changed = tx
        .execute(
            "UPDATE plugins SET active=? WHERE id=?",
            params![to.map(|id| signed(id.0)).transpose()?, plugin.to_string()],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(backend(format!("unknown plugin {plugin}")));
    }
    tx.commit().map_err(backend)
}
