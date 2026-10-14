use super::{backend, encode, rows, signed};
use crate::sqlite::timestamp;
use enco_core::*;
use enco_kernel::{NewGeneration, RegistryState, StoreError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) fn read(connection: &Connection) -> Result<RegistryState, StoreError> {
    let mut generations = connection.prepare("SELECT g.id,p.plugin_id,p.artifact,p.settings,p.origin,p.status,COALESCE(p.failure,'null'),g.created_at FROM plugin_generations p JOIN generations g ON g.id=p.id ORDER BY g.id").map_err(backend)?;
    let generations = generations
        .query_map([], |row| {
            Ok(GenerationRecord {
                id: GenerationId(rows::unsigned(row, 0)?),
                plugin: rows::text(row, 1)?,
                artifact: rows::text(row, 2)?,
                settings: rows::document(row, 3)?,
                origin: rows::text(row, 4)?,
                status: rows::text(row, 5)?,
                failure: rows::document(row, 6)?,
                created_at: rows::text(row, 7)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let mut active = connection
        .prepare("SELECT id,active FROM plugins WHERE active IS NOT NULL ORDER BY id")
        .map_err(backend)?;
    let active = active
        .query_map([], |row| {
            Ok((rows::text(row, 0)?, GenerationId(rows::unsigned(row, 1)?)))
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    let mut configs = connection.prepare("SELECT g.id,c.document,g.created_at FROM config_generations c JOIN generations g ON g.id=c.id ORDER BY g.id").map_err(backend)?;
    let configs = configs
        .query_map([], |row| {
            Ok(ConfigGeneration {
                id: GenerationId(rows::unsigned(row, 0)?),
                document: rows::document(row, 1)?,
                created_at: rows::text(row, 2)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    let active_config = connection
        .query_row("SELECT active FROM config WHERE singleton=1", [], |row| {
            Ok(GenerationId(rows::unsigned(row, 0)?))
        })
        .optional()
        .map_err(backend)?;
    Ok(RegistryState {
        generations,
        active,
        config_generations: configs,
        active_config,
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
    let id = insert_in(&tx, generation, activate)?;
    tx.commit().map_err(backend)?;
    Ok(id)
}

fn insert_in(
    tx: &Connection,
    generation: &NewGeneration,
    activate: bool,
) -> Result<GenerationId, StoreError> {
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
        GenerationStatus::Trial => "trial",
        GenerationStatus::Healthy => "healthy",
        GenerationStatus::Failed => "failed",
    };
    tx.execute(
        "INSERT INTO generations(created_at) VALUES (?)",
        [timestamp(generation.created_at)],
    )
    .map_err(backend)?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO plugin_generations(id,plugin_id,artifact,settings,origin,status) VALUES (?,?,?,?,?,?)", params![id, plugin, generation.artifact.to_string(), encode(&generation.settings)?, origin, status]).map_err(backend)?;
    if activate {
        tx.execute(
            "UPDATE plugins SET active=? WHERE id=?",
            params![id, plugin],
        )
        .map_err(backend)?;
    }
    Ok(GenerationId(u64::try_from(id).map_err(backend)?))
}

pub(super) fn activate(
    connection: &mut Connection,
    plugin: PluginId,
    to: Option<GenerationId>,
    failed: &[(GenerationId, Failure)],
    event: Option<&Event>,
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    for (id, failure) in failed {
        let changed = tx
            .execute(
                "UPDATE plugin_generations SET status='failed',failure=? WHERE id=? AND plugin_id=?",
                params![encode(failure)?, signed(id.0)?, plugin.to_string()],
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
    if let Some(event) = event {
        super::accept::events_in(&tx, std::slice::from_ref(event))?;
    }
    tx.commit().map_err(backend)
}

pub(super) fn promote(connection: &Connection, generation: GenerationId) -> Result<(), StoreError> {
    let changed = connection
        .execute(
            "UPDATE plugin_generations SET status='healthy' WHERE id=? AND status='trial'",
            [signed(generation.0)?],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(backend(format!(
            "generation {generation} is absent or not trial"
        )));
    }
    Ok(())
}

pub(super) fn insert_config_generation(
    connection: &mut Connection,
    document: &serde_json::Value,
    created_at: DateTime<Utc>,
    plugins: &[NewGeneration],
) -> Result<(GenerationId, Vec<GenerationId>), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    tx.execute(
        "INSERT INTO generations(created_at) VALUES (?)",
        [timestamp(created_at)],
    )
    .map_err(backend)?;
    let id = GenerationId(u64::try_from(tx.last_insert_rowid()).map_err(backend)?);
    tx.execute(
        "INSERT INTO config_generations(id,document) VALUES (?,?)",
        params![signed(id.0)?, encode(document)?],
    )
    .map_err(backend)?;
    activate_config(&tx, id)?;
    let plugins = plugins
        .iter()
        .map(|generation| insert_in(&tx, generation, true))
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().map_err(backend)?;
    Ok((id, plugins))
}

pub(super) fn activate_config(
    connection: &Connection,
    generation: GenerationId,
) -> Result<(), StoreError> {
    connection.execute("INSERT INTO config(singleton,active) VALUES (1,?) ON CONFLICT(singleton) DO UPDATE SET active=excluded.active", [signed(generation.0)?]).map_err(backend)?;
    Ok(())
}
