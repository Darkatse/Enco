//! Reconstruct typed registry facts from their durable rows.
use super::{activation, backend, code};
use crate::sqlite::optional_unsigned;
use crate::store::rows;
use enco_core::*;
use enco_kernel::{AcceptedIntent, NodeState, RegistryState, Replaced, StoreError};
use rusqlite::{Connection, OptionalExtension, Row};
use std::collections::BTreeMap;

pub(in crate::store) fn read(connection: &Connection) -> Result<RegistryState, StoreError> {
    let registrations = registrations(connection)?;
    Ok(RegistryState {
        node: node(connection, &registrations)?,
        intent: AcceptedIntent {
            generations: query(
                connection,
                "SELECT registration, plugin_id, artifact, settings,
                        based_on_generation, based_on_factory
                 FROM plugin_generations ORDER BY registration, plugin_id",
                |row| {
                    let plugin = rows::text(row, 1)?;
                    let id = GenerationId {
                        registration: RegistrationId(rows::unsigned(row, 0)?),
                        plugin,
                    };
                    Ok((
                        id,
                        GenerationRecord {
                            id,
                            artifact: rows::text(row, 2)?,
                            settings: rows::document(row, 3)?,
                            origin: match code(row, plugin, 4)? {
                                None => Origin::Code,
                                Some(based_on) => Origin::Settings { based_on },
                            },
                        },
                    ))
                },
            )?,
            configurations: query(
                connection,
                "SELECT registration, document, source FROM config_generations ORDER BY registration",
                |row| {
                    let registration = RegistrationId(rows::unsigned(row, 0)?);
                    Ok((
                        registration,
                        ConfigGeneration {
                            registration,
                            document: rows::document(row, 1)?,
                            source: rows::text(row, 2)?,
                        },
                    ))
                },
            )?,
            registrations,
        },
    })
}

/// This node's references, health, configuration and what each registration replaced here.
fn node(
    connection: &Connection,
    registrations: &BTreeMap<RegistrationId, RegistrationRecord>,
) -> Result<NodeState, StoreError> {
    let mut replaced: BTreeMap<_, _> = registrations
        .keys()
        .map(|id| (*id, Replaced::default()))
        .collect();
    let names = query::<_, Vec<_>>(
        connection,
        "SELECT registration, plugin_id, previous FROM replaced_names",
        |row| {
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                rows::text(row, 1)?,
                optional_unsigned(row, 2)?.map(RegistrationId),
            ))
        },
    )?;
    for (registration, plugin, previous) in names {
        replaced
            .get_mut(&registration)
            .ok_or_else(|| backend(format!("missing registration {registration}")))?
            .names
            .insert(plugin, previous);
    }
    let configs = query::<_, Vec<_>>(
        connection,
        "SELECT registration, previous FROM replaced_config",
        |row| {
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                RegistrationId(rows::unsigned(row, 1)?),
            ))
        },
    )?;
    for (registration, previous) in configs {
        replaced
            .get_mut(&registration)
            .ok_or_else(|| backend(format!("missing registration {registration}")))?
            .configuration = Some(previous);
    }
    Ok(NodeState {
        plugins: query(
            connection,
            "SELECT id, named_by, active, active_factory, active_by,
                    rollback_target, rollback_factory, rollback_by
             FROM plugins",
            |row| {
                let id = rows::text(row, 0)?;
                Ok((
                    id,
                    PluginRecord {
                        id,
                        named_by: optional_unsigned(row, 1)?.map(RegistrationId),
                        active: activation(row, id, 2)?,
                        rollback_target: activation(row, id, 5)?,
                    },
                ))
            },
        )?,
        health: query(
            connection,
            "SELECT registration, plugin_id, status, COALESCE(failure, 'null')
             FROM generation_health",
            |row| {
                let id = GenerationId {
                    registration: RegistrationId(rows::unsigned(row, 0)?),
                    plugin: rows::text(row, 1)?,
                };
                let health = match row.get::<_, String>(2)?.as_str() {
                    "trial" => Health::Trial,
                    "healthy" => Health::Healthy,
                    "failed" => Health::Failed {
                        failure: rows::document(row, 3)?,
                    },
                    kind => return Err(invalid_kind(2, kind)),
                };
                Ok((id, health))
            },
        )?,
        configuration: connection
            .query_row("SELECT active FROM config WHERE singleton=1", [], |row| {
                Ok(RegistrationId(rows::unsigned(row, 0)?))
            })
            .optional()
            .map_err(backend)?,
        replaced,
    })
}

fn registrations(
    connection: &Connection,
) -> Result<BTreeMap<RegistrationId, RegistrationRecord>, StoreError> {
    let mut registrations: BTreeMap<_, _> = query(
        connection,
        "SELECT id,initiator,created_at,session,call,reason FROM registrations ORDER BY id",
        |row| {
            let id = RegistrationId(rows::unsigned(row, 0)?);
            Ok((
                id,
                RegistrationRecord {
                    id,
                    initiator: initiator(row)?,
                    names: BTreeMap::new(),
                    code: BTreeMap::new(),
                    created_at: rows::text(row, 2)?,
                },
            ))
        },
    )?;
    let names = query::<_, Vec<_>>(
        connection,
        "SELECT registration,plugin_id,name FROM registration_names ORDER BY registration",
        |row| {
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                rows::text(row, 1)?,
                RegisteredName {
                    name: crate::sqlite::optional_text(row, 2)?,
                },
            ))
        },
    )?;
    for (id, plugin, name) in names {
        registrations
            .get_mut(&id)
            .ok_or_else(|| backend(format!("missing registration {id}")))?
            .names
            .insert(plugin, name);
    }
    let code = query::<_, Vec<_>>(
        connection,
        "SELECT registration,plugin_id,generation,factory,intent,source
         FROM registration_code ORDER BY registration",
        |row| {
            let plugin = rows::text(row, 1)?;
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                plugin,
                RegisteredCode {
                    code: code(row, plugin, 2)?.ok_or_else(|| invalid_kind(2, "no code"))?,
                    intent: match row.get::<_, Option<String>>(4)?.as_deref() {
                        Some("directory") => Some(CodeIntent::Directory {
                            source: rows::text(row, 5)?,
                        }),
                        Some("factory") => Some(CodeIntent::Factory),
                        None => None,
                        Some(kind) => return Err(invalid_kind(4, kind)),
                    },
                },
            ))
        },
    )?;
    for (id, plugin, code) in code {
        registrations
            .get_mut(&id)
            .ok_or_else(|| backend(format!("missing registration {id}")))?
            .code
            .insert(plugin, code);
    }
    Ok(registrations)
}

fn query<T, C: FromIterator<T>>(
    connection: &Connection,
    sql: &str,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<C, StoreError> {
    connection
        .prepare(sql)
        .map_err(backend)?
        .query_map([], map)
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)
}

fn initiator(row: &Row<'_>) -> rusqlite::Result<Initiator> {
    Ok(match row.get::<_, String>(1)?.as_str() {
        "owner" => Initiator::Owner {
            reason: crate::sqlite::optional_text(row, 5)?,
        },
        "host" => Initiator::Host,
        "agent" => Initiator::Agent {
            session: rows::text(row, 3)?,
            call: rows::text(row, 4)?,
            reason: rows::text(row, 5)?,
        },
        kind => return Err(invalid_kind(1, kind)),
    })
}

fn invalid_kind(column: usize, kind: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("unknown registry kind {kind}").into(),
    )
}
