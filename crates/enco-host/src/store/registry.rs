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
fn columns(plugin: PluginId, code: &CodeRef) -> Result<(Option<i64>, Option<String>), StoreError> {
    match code {
        CodeRef::Generation { id } if id.plugin == plugin => {
            Ok((Some(signed(id.registration.0)?), None))
        }
        CodeRef::Factory {
            plugin: owner,
            artifact,
        } if *owner == plugin => Ok((None, Some(artifact.to_string()))),
        other => Err(backend(format!("plugin {plugin} cannot run {other:?}"))),
    }
}

/// The code two columns name for a plugin, absent when both are empty.
fn code(row: &Row<'_>, plugin: PluginId, column: usize) -> rusqlite::Result<Option<CodeRef>> {
    let generation = optional_unsigned(row, column)?;
    let factory: Option<ContentHash> = optional_text(row, column + 1)?;
    Ok(match (generation, factory) {
        (Some(registration), _) => Some(CodeRef::from(GenerationId {
            registration: RegistrationId(registration),
            plugin,
        })),
        (None, Some(artifact)) => Some(CodeRef::Factory { plugin, artifact }),
        (None, None) => None,
    })
}

/// A reference as three columns: its code, then the registration that wrote it.
fn activation(
    row: &Row<'_>,
    plugin: PluginId,
    column: usize,
) -> rusqlite::Result<Option<PluginActivation>> {
    let Some(code) = code(row, plugin, column)? else {
        return Ok(None);
    };
    Ok(Some(PluginActivation {
        code,
        registration: optional_unsigned(row, column + 2)?.map(RegistrationId),
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
        // A registration activates the generation it brought, or factory code.
        let factory = match &registered.code {
            CodeRef::Generation { id } if id.registration == record.id && id.plugin == *plugin => {
                None
            }
            CodeRef::Factory {
                plugin: owner,
                artifact,
            } if owner == plugin => Some(artifact.to_string()),
            other => {
                return Err(backend(format!(
                    "registration {} cannot activate {other:?} for {plugin}",
                    record.id
                )));
            }
        };
        let (intent, source) = match registered.intent {
            Some(CodeIntent::Directory { source }) => (Some("directory"), Some(source.to_string())),
            Some(CodeIntent::Factory) => (Some("factory"), None),
            None => (None, None),
        };
        tx.execute(
            "INSERT INTO registration_code(registration,plugin_id,factory,intent,source) VALUES (?,?,?,?,?)",
            params![registration, plugin.to_string(), factory, intent, source],
        ).map_err(backend)?;
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
    let (based_on_generation, based_on_factory) = match &record.origin {
        Origin::Code => (None, None),
        Origin::Settings { based_on } => columns(record.id.plugin, based_on)?,
    };
    let key = params![
        signed(record.id.registration.0)?,
        record.id.plugin.to_string()
    ];
    connection
        .execute(
            "INSERT INTO plugin_generations(registration,plugin_id,artifact,settings,based_on_generation,based_on_factory)
             VALUES (?,?,?,?,?,?)",
            params![
                signed(record.id.registration.0)?,
                record.id.plugin.to_string(),
                record.artifact.to_string(),
                encode(&record.settings)?,
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
        let reference = |activation: &Option<PluginActivation>| -> Result<_, StoreError> {
            let Some(activation) = activation else {
                return Ok((None, None, None));
            };
            let (generation, factory) = columns(plugin.id, &activation.code)?;
            let by = activation.registration.map(|id| signed(id.0)).transpose()?;
            Ok((generation, factory, by))
        };
        let (active, active_factory, active_by) = reference(&plugin.active)?;
        let (rollback, rollback_factory, rollback_by) = reference(&plugin.rollback_target)?;
        let changed = connection
            .execute(
                "UPDATE plugins SET named_by=?,active=?,active_factory=?,active_by=?,
                     rollback_target=?,rollback_factory=?,rollback_by=?
                 WHERE id=?",
                params![
                    plugin.named_by.map(|id| signed(id.0)).transpose()?,
                    active,
                    active_factory,
                    active_by,
                    rollback,
                    rollback_factory,
                    rollback_by,
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
         WHERE (p.active IS NOT NULL OR p.active_factory IS NOT NULL) AND n.name IS NOT NULL
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
