//! Registration writes and mechanical activation share the registry's durable references.
use super::{backend, encode, rows, signed};
use crate::sqlite::{optional_unsigned, timestamp};
use enco_core::*;
use enco_kernel::{ActivationWrite, RegistrationWrite, StoreError};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
mod read;
pub(super) use read::read;

pub(super) fn record_factories(
    connection: &mut Connection,
    generations: &[GenerationRecord],
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    for record in generations {
        tx.execute(
            "INSERT INTO plugins(id) VALUES (?) ON CONFLICT(id) DO NOTHING",
            [record.plugin.to_string()],
        )
        .map_err(backend)?;
        insert_generation(&tx, record)?;
    }
    tx.commit().map_err(backend)
}

fn activation(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<PluginActivation>> {
    let Some(generation) = optional_unsigned(row, column)? else {
        return Ok(None);
    };
    Ok(Some(PluginActivation {
        generation: GenerationId(generation),
        registration: optional_unsigned(row, column + 1)?.map(RegistrationId),
    }))
}

pub(super) fn register(
    connection: &mut Connection,
    write: &RegistrationWrite,
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let record = &write.registration;
    let (initiator, session, call) = match &record.initiator {
        Initiator::Owner { .. } => ("owner", None, None),
        Initiator::Host => ("host", None, None),
        Initiator::Agent { session, call, .. } => {
            ("agent", Some(session.to_string()), Some(call.to_string()))
        }
    };
    tx.execute(
        "INSERT INTO registrations(id,initiator,session,call,reason,created_at) VALUES (?,?,?,?,?,?)",
        params![
            signed(record.id.0)?,
            initiator, session, call, record.initiator.reason().map(Reason::as_str),
            timestamp(record.created_at)
        ],
    )
    .map_err(backend)?;
    for plugin in &write.plugins {
        tx.execute(
            "INSERT INTO plugins(id) VALUES (?) ON CONFLICT(id) DO NOTHING",
            [plugin.id.to_string()],
        )
        .map_err(backend)?;
    }
    for (plugin, name) in &record.names {
        let current: Option<u64> = tx
            .query_row(
                "SELECT named_by FROM plugins WHERE id=?",
                [plugin.to_string()],
                |row| optional_unsigned(row, 0),
            )
            .map_err(backend)?;
        if current.map(RegistrationId) != name.previous {
            return Err(backend(format!(
                "plugin {plugin}: name changed before registration {}",
                record.id
            )));
        }
        tx.execute(
            "INSERT INTO registration_names(registration,plugin_id,name,previous) VALUES (?,?,?,?)",
            params![
                signed(record.id.0)?,
                plugin.to_string(),
                name.name.as_deref(),
                name.previous.map(|id| signed(id.0)).transpose()?
            ],
        )
        .map_err(backend)?;
    }
    for (plugin, code) in &record.code {
        let (intent, source) = match code.intent {
            Some(CodeIntent::Directory { source }) => (Some("directory"), Some(source.to_string())),
            Some(CodeIntent::Factory) => (Some("factory"), None),
            None => (None, None),
        };
        tx.execute(
            "INSERT INTO registration_code(registration,plugin_id,generation,intent,source) VALUES (?,?,?,?,?)",
            params![signed(record.id.0)?, plugin.to_string(), signed(code.generation.0)?, intent, source],
        ).map_err(backend)?;
    }
    for generation in &write.generations {
        insert_generation(&tx, generation)?;
    }
    if let Some(config) = &write.configuration {
        insert_config(&tx, config)?;
    }
    references(
        &tx,
        &write.plugins,
        write.configuration.as_ref().map(|record| record.id),
    )?;
    tx.commit().map_err(backend)
}

fn insert_generation(connection: &Connection, record: &GenerationRecord) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO generations(id,created_at) VALUES (?,?)",
            params![signed(record.id.0)?, timestamp(record.created_at)],
        )
        .map_err(backend)?;
    let (origin, registration, based_on) = match record.origin {
        Origin::Factory => ("factory", None, None),
        Origin::Code { registration } => ("code", Some(registration), None),
        Origin::Settings {
            registration,
            based_on,
        } => ("settings", Some(registration), Some(based_on)),
    };
    let status = match record.status {
        GenerationStatus::Trial => "trial",
        GenerationStatus::Healthy => "healthy",
        GenerationStatus::Failed => "failed",
    };
    connection
        .execute(
            "INSERT INTO plugin_generations(id,plugin_id,artifact,settings,origin,status,failure,registration,based_on)
         VALUES (?,?,?,?,?,?,?,?,?)",
            params![
                signed(record.id.0)?,
                record.plugin.to_string(),
                record.artifact.to_string(),
                encode(&record.settings)?,
                origin,
                status,
                record.failure.as_ref().map(encode).transpose()?,
                registration.map(|id| signed(id.0)).transpose()?,
                based_on.map(|id| signed(id.0)).transpose()?
            ],
        )
        .map_err(backend)?;
    Ok(())
}

fn insert_config(connection: &Connection, record: &ConfigGeneration) -> Result<(), StoreError> {
    let current: Option<u64> = connection
        .query_row("SELECT active FROM config WHERE singleton=1", [], |row| {
            rows::unsigned(row, 0)
        })
        .optional()
        .map_err(backend)?;
    if current.map(GenerationId) != record.previous {
        return Err(backend("active configuration changed before registration"));
    }
    connection
        .execute(
            "INSERT INTO generations(id,created_at) VALUES (?,?)",
            params![signed(record.id.0)?, timestamp(record.created_at)],
        )
        .map_err(backend)?;
    connection
        .execute(
            "INSERT INTO config_generations(id,registration,document,source,previous) VALUES (?,?,?,?,?)",
            params![
                signed(record.id.0)?,
                signed(record.registration.0)?,
                encode(&record.document)?,
                record.source.to_string(),
                record.previous.map(|id| signed(id.0)).transpose()?
            ],
        )
        .map_err(backend)?;
    Ok(())
}

pub(super) fn activate(
    connection: &mut Connection,
    change: &ActivationWrite,
) -> Result<(), StoreError> {
    let events = super::accept::encode_events(change.event.as_slice())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    for (id, failure) in &change.failed {
        if tx
            .execute(
                "UPDATE plugin_generations SET status='failed',failure=? WHERE id=?",
                params![encode(failure)?, signed(id.0)?],
            )
            .map_err(backend)?
            != 1
        {
            return Err(StoreError::UnknownGeneration(*id));
        }
    }
    references(&tx, &change.plugins, change.configuration)?;
    super::accept::events_in(&tx, &events)?;
    tx.commit().map_err(backend)
}

fn references(
    connection: &Connection,
    plugins: &[PluginRecord],
    config: Option<GenerationId>,
) -> Result<(), StoreError> {
    for plugin in plugins {
        let changed = connection
            .execute(
                "UPDATE plugins SET named_by=?,active=?,active_by=?,rollback_target=?,rollback_by=? WHERE id=?",
                params![
                    plugin.named_by.map(|id| signed(id.0)).transpose()?,
                    plugin.active.map(|active| signed(active.generation.0)).transpose()?,
                    plugin.active.and_then(|active| active.registration).map(|id| signed(id.0)).transpose()?,
                    plugin.rollback_target.map(|active| signed(active.generation.0)).transpose()?,
                    plugin.rollback_target.and_then(|active| active.registration).map(|id| signed(id.0)).transpose()?,
                    plugin.id.to_string()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(backend(format!("unknown plugin {}", plugin.id)));
        }
    }
    if let Some(id) = config {
        connection
            .execute(
                "INSERT INTO config(singleton,active) VALUES (1,?)
             ON CONFLICT(singleton) DO UPDATE SET active=excluded.active",
                [signed(id.0)?],
            )
            .map_err(backend)?;
    }
    // Durable names must stay unique after both registration and mechanical recovery.
    let conflict: Option<String> = connection
        .query_row(
            "SELECT n.name FROM plugins p
         JOIN registration_names n ON n.registration=p.named_by AND n.plugin_id=p.id
         WHERE p.active IS NOT NULL AND n.name IS NOT NULL
         GROUP BY n.name HAVING COUNT(*)>1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(name) = conflict {
        return Err(backend(format!("running plugin name {name} is not unique")));
    }
    Ok(())
}

pub(super) fn promote(connection: &Connection, generation: GenerationId) -> Result<(), StoreError> {
    if connection
        .execute(
            "UPDATE plugin_generations SET status='healthy' WHERE id=? AND status='trial'",
            [signed(generation.0)?],
        )
        .map_err(backend)?
        != 1
    {
        return Err(backend(format!(
            "generation {generation} is absent or not trial"
        )));
    }
    Ok(())
}
