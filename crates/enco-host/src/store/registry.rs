//! Registration writes and mechanical activation: accepted intent in the control-plane tables,
//! this node's references and health in its own.
use super::{backend, encode, signed};
use crate::sqlite::{optional_text, optional_unsigned, timestamp};
use enco_core::*;
use enco_kernel::{ActivationWrite, RegistrationWrite, StoreError};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
mod read;
pub(super) use read::read;

/// A plugin's code as two columns: a generation's registration, or factory code's artifact.
pub(super) fn columns(code: PluginCode) -> Result<(Option<i64>, Option<String>), StoreError> {
    match code {
        PluginCode::Generation { registration } => Ok((Some(signed(registration.0)?), None)),
        PluginCode::Factory { artifact } => Ok((None, Some(artifact.to_string()))),
    }
}

/// The plugin's code two columns name, absent when both are empty.
fn code(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<PluginCode>> {
    let generation = optional_unsigned(row, column)?;
    let factory: Option<ContentHash> = optional_text(row, column + 1)?;
    Ok(match (generation, factory) {
        (Some(registration), _) => Some(PluginCode::Generation {
            registration: RegistrationId(registration),
        }),
        (None, Some(artifact)) => Some(PluginCode::Factory { artifact }),
        (None, None) => None,
    })
}

/// A reference as two columns: the registration that wrote it, and factory code's artifact.
pub(super) fn reference(
    activation: Option<PluginActivation>,
) -> Result<(Option<i64>, Option<String>), StoreError> {
    Ok(match activation {
        None => (None, None),
        Some(PluginActivation::Generation { registration }) => {
            (Some(signed(registration.0)?), None)
        }
        Some(PluginActivation::Factory {
            artifact,
            registration,
        }) => (
            registration.map(|id| signed(id.0)).transpose()?,
            Some(artifact.to_string()),
        ),
    })
}

/// The reference two columns name, absent when both are empty.
fn activation(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<PluginActivation>> {
    let registration = optional_unsigned(row, column)?.map(RegistrationId);
    let factory: Option<ContentHash> = optional_text(row, column + 1)?;
    Ok(match (registration, factory) {
        (None, None) => None,
        (Some(registration), None) => Some(PluginActivation::Generation { registration }),
        (registration, Some(artifact)) => Some(PluginActivation::Factory {
            artifact,
            registration,
        }),
    })
}

pub(super) fn register(
    connection: &mut Connection,
    write: &RegistrationWrite,
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(backend)?;
    let record = &write.registration;
    let registration = signed(record.id.0)?;
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
            registration,
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
    names(&tx, write)?;
    for (plugin, registered) in &record.code {
        let factory = match registered {
            RegisteredCode::Generation => None,
            RegisteredCode::Factory { artifact } => Some(artifact.to_string()),
        };
        tx.execute(
            "INSERT INTO registration_code(registration,plugin_id,factory) VALUES (?,?,?)",
            params![registration, plugin.to_string(), factory],
        )
        .map_err(backend)?;
    }
    for generation in &write.generations {
        insert_generation(&tx, generation)?;
    }
    if let Some(config) = &write.configuration {
        insert_config(&tx, config, write.replaced.configuration)?;
    }
    references(
        &tx,
        &write.plugins,
        write
            .configuration
            .as_ref()
            .map(|record| record.registration),
    )?;
    tx.commit().map_err(backend)
}

/// Names this registration wrote, each checked against the name it replaces on this node.
fn names(connection: &Connection, write: &RegistrationWrite) -> Result<(), StoreError> {
    let record = &write.registration;
    for (plugin, name) in &record.names {
        let previous = write.replaced.names.get(plugin).copied().flatten();
        let current: Option<u64> = connection
            .query_row(
                "SELECT named_by FROM plugins WHERE id=?",
                [plugin.to_string()],
                |row| optional_unsigned(row, 0),
            )
            .map_err(backend)?;
        if current.map(RegistrationId) != previous {
            return Err(backend(format!(
                "plugin {plugin}: name changed before registration {}",
                record.id
            )));
        }
        connection
            .execute(
                "INSERT INTO registration_names(registration,plugin_id,name) VALUES (?,?,?)",
                params![
                    signed(record.id.0)?,
                    plugin.to_string(),
                    name.name.as_deref()
                ],
            )
            .map_err(backend)?;
        connection
            .execute(
                "INSERT INTO replaced_names(registration,plugin_id,previous) VALUES (?,?,?)",
                params![
                    signed(record.id.0)?,
                    plugin.to_string(),
                    previous.map(|id| signed(id.0)).transpose()?
                ],
            )
            .map_err(backend)?;
    }
    Ok(())
}

/// Content is intent; its trial on this node is the node's fact, written with it.
fn insert_generation(connection: &Connection, record: &GenerationRecord) -> Result<(), StoreError> {
    let (intent, source, (based_on_generation, based_on_factory)) = match &record.origin {
        Origin::Code {
            intent: CodeIntent::Directory { source },
        } => (Some("directory"), Some(source.to_string()), (None, None)),
        Origin::Code {
            intent: CodeIntent::Factory,
        } => (Some("factory"), None, (None, None)),
        Origin::Settings { based_on } => (None, None, columns(*based_on)?),
    };
    let key = params![
        signed(record.id.registration.0)?,
        record.id.plugin.to_string()
    ];
    connection
        .execute(
            "INSERT INTO plugin_generations(registration,plugin_id,artifact,settings,intent,source,
                 based_on_generation,based_on_factory)
             VALUES (?,?,?,?,?,?,?,?)",
            params![
                signed(record.id.registration.0)?,
                record.id.plugin.to_string(),
                record.artifact.to_string(),
                encode(&record.settings)?,
                intent,
                source,
                based_on_generation,
                based_on_factory
            ],
        )
        .map_err(backend)?;
    connection
        .execute(
            "INSERT INTO generation_health(registration,plugin_id,status) VALUES (?,?,'trial')",
            key,
        )
        .map_err(backend)?;
    Ok(())
}

/// A new configuration replaces the one that runs on this node, which must be the one expected.
fn insert_config(
    connection: &Connection,
    record: &ConfigGeneration,
    previous: Option<RegistrationId>,
) -> Result<(), StoreError> {
    let current: Option<u64> = connection
        .query_row("SELECT active FROM config WHERE singleton=1", [], |row| {
            crate::sqlite::unsigned(row, 0)
        })
        .optional()
        .map_err(backend)?;
    if current.map(RegistrationId) != previous {
        return Err(backend("active configuration changed before registration"));
    }
    let registration = signed(record.registration.0)?;
    connection
        .execute(
            "INSERT INTO config_generations(registration,document,source) VALUES (?,?,?)",
            params![
                registration,
                encode(&record.document)?,
                record.source.to_string()
            ],
        )
        .map_err(backend)?;
    if let Some(previous) = previous {
        connection
            .execute(
                "INSERT INTO replaced_config(registration,previous) VALUES (?,?)",
                params![registration, signed(previous.0)?],
            )
            .map_err(backend)?;
    }
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
                "UPDATE generation_health SET status='failed',failure=?
                 WHERE registration=? AND plugin_id=?",
                params![
                    encode(failure)?,
                    signed(id.registration.0)?,
                    id.plugin.to_string()
                ],
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
    config: Option<RegistrationId>,
) -> Result<(), StoreError> {
    for plugin in plugins {
        let (active_by, active_factory) = reference(plugin.active)?;
        let (rollback_by, rollback_factory) = reference(plugin.rollback_target)?;
        let changed = connection
            .execute(
                "UPDATE plugins SET named_by=?,active_by=?,active_factory=?,rollback_by=?,
                     rollback_factory=?
                 WHERE id=?",
                params![
                    plugin.named_by.map(|id| signed(id.0)).transpose()?,
                    active_by,
                    active_factory,
                    rollback_by,
                    rollback_factory,
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
         WHERE (p.active_by IS NOT NULL OR p.active_factory IS NOT NULL) AND n.name IS NOT NULL
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
            "UPDATE generation_health SET status='healthy'
             WHERE registration=? AND plugin_id=? AND status='trial'",
            params![
                signed(generation.registration.0)?,
                generation.plugin.to_string()
            ],
        )
        .map_err(backend)?
        != 1
    {
        return Err(backend(format!("{generation} is absent or not on trial")));
    }
    Ok(())
}
