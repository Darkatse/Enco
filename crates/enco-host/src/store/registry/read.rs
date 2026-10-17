//! Reconstruct typed registry facts from their durable rows.
use super::{activation, backend, rows};
use crate::sqlite::optional_unsigned;
use enco_core::*;
use enco_kernel::{RegistryState, StoreError};
use rusqlite::{Connection, OptionalExtension, Row};
use std::collections::BTreeMap;

pub(in crate::store) fn read(connection: &Connection) -> Result<RegistryState, StoreError> {
    Ok(RegistryState {
        registrations: registrations(connection)?,
        plugins: query(
            connection,
            "SELECT id,named_by,active,active_by,rollback_target,rollback_by FROM plugins",
            |row| {
                let id = rows::text(row, 0)?;
                Ok((
                    id,
                    PluginRecord {
                        id,
                        named_by: optional_unsigned(row, 1)?.map(RegistrationId),
                        active: activation(row, 2)?,
                        rollback_target: activation(row, 4)?,
                    },
                ))
            },
        )?,
        generations: query(
            connection,
            "SELECT g.id, p.registration, p.plugin_id, p.artifact, p.settings,
                    p.origin, p.status, COALESCE(p.failure, 'null'), g.created_at, p.based_on
             FROM plugin_generations p JOIN generations g ON g.id = p.id
             ORDER BY g.id",
            |row| {
                let id = GenerationId(rows::unsigned(row, 0)?);
                Ok((
                    id,
                    GenerationRecord {
                        id,
                        plugin: rows::text(row, 2)?,
                        artifact: rows::text(row, 3)?,
                        settings: rows::document(row, 4)?,
                        origin: origin(row)?,
                        status: rows::text(row, 6)?,
                        failure: rows::document(row, 7)?,
                        created_at: rows::text(row, 8)?,
                    },
                ))
            },
        )?,
        config_generations: query(
            connection,
            "SELECT g.id, c.registration, c.document, c.previous, g.created_at, c.source
             FROM config_generations c JOIN generations g ON g.id = c.id
             ORDER BY g.id",
            |row| {
                let id = GenerationId(rows::unsigned(row, 0)?);
                Ok((
                    id,
                    ConfigGeneration {
                        id,
                        registration: RegistrationId(rows::unsigned(row, 1)?),
                        document: rows::document(row, 2)?,
                        source: rows::text(row, 5)?,
                        previous: optional_unsigned(row, 3)?.map(GenerationId),
                        created_at: rows::text(row, 4)?,
                    },
                ))
            },
        )?,
        active_config: connection
            .query_row("SELECT active FROM config WHERE singleton=1", [], |row| {
                Ok(GenerationId(rows::unsigned(row, 0)?))
            })
            .optional()
            .map_err(backend)?,
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
        "SELECT registration,plugin_id,name,previous FROM registration_names ORDER BY registration",
        |row| {
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                rows::text(row, 1)?,
                RegisteredName {
                    name: crate::sqlite::optional_text(row, 2)?,
                    previous: optional_unsigned(row, 3)?.map(RegistrationId),
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
        "SELECT registration,plugin_id,generation,intent,source FROM registration_code ORDER BY registration",
        |row| {
            Ok((
                RegistrationId(rows::unsigned(row, 0)?),
                rows::text(row, 1)?,
                RegisteredCode {
                    generation: GenerationId(rows::unsigned(row, 2)?),
                    intent: match row.get::<_, Option<String>>(3)?.as_deref() {
                        Some("directory") => Some(CodeIntent::Directory {
                            source: rows::text(row, 4)?,
                        }),
                        Some("factory") => Some(CodeIntent::Factory),
                        None => None,
                        Some(kind) => return Err(invalid_kind(3, kind)),
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

fn origin(row: &Row<'_>) -> rusqlite::Result<Origin> {
    Ok(match row.get::<_, String>(5)?.as_str() {
        "factory" => Origin::Factory,
        "code" => Origin::Code {
            registration: RegistrationId(rows::unsigned(row, 1)?),
        },
        "artifact" => Origin::Artifact {
            registration: RegistrationId(rows::unsigned(row, 1)?),
        },
        "settings" => Origin::Settings {
            registration: RegistrationId(rows::unsigned(row, 1)?),
            based_on: GenerationId(rows::unsigned(row, 9)?),
        },
        kind => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                format!("unknown generation origin {kind}").into(),
            ));
        }
    })
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
