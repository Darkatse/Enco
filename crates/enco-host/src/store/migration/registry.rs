//! The version 23 registry, read before its tables are replaced.
//!
//! Every old generation number resolves to the code or configuration it names, and the rows are
//! written again in the version 24 tables.
use super::super::registry::{columns, reference};
use super::super::{backend, rows, signed};
use crate::sqlite::optional_unsigned;
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::{Connection, Row, params};
use std::collections::BTreeMap;

/// A plugin generation as version 23 recorded it; factory generations have no registration.
struct Generation {
    plugin: PluginId,
    artifact: ContentHash,
    settings: String,
    registration: Option<u64>,
    based_on: Option<u64>,
    status: String,
    failure: Option<String>,
}

struct Configuration {
    registration: u64,
    document: String,
    source: String,
    previous: Option<u64>,
}

/// A reference as version 23 recorded it: a generation number and the registration that wrote it.
type Reference = (Option<u64>, Option<u64>);

struct Plugin {
    id: PluginId,
    named_by: Option<u64>,
    active: Reference,
    rollback: Reference,
}

struct Name {
    registration: u64,
    plugin: PluginId,
    name: Option<String>,
    previous: Option<u64>,
}

struct Code {
    registration: u64,
    plugin: PluginId,
    generation: u64,
    intent: Option<String>,
    source: Option<String>,
}

/// The version 23 registry rows that version 24 keeps in other tables.
pub(super) struct Registry {
    generations: BTreeMap<u64, Generation>,
    configurations: BTreeMap<u64, Configuration>,
    plugins: Vec<Plugin>,
    names: Vec<Name>,
    code: Vec<Code>,
    config: Option<u64>,
}

fn rows_of<T>(
    connection: &Connection,
    sql: &str,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, StoreError> {
    connection
        .prepare(sql)
        .map_err(backend)?
        .query_map([], map)
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)
}

fn unknown(id: u64) -> StoreError {
    backend(format!("generation {id} is not recorded"))
}

impl Registry {
    pub fn read(connection: &Connection) -> Result<Self, StoreError> {
        let generations = rows_of(
            connection,
            "SELECT id,plugin_id,artifact,settings,registration,based_on,status,failure
             FROM plugin_generations ORDER BY id",
            |row| {
                Ok((
                    rows::unsigned(row, 0)?,
                    Generation {
                        plugin: rows::text(row, 1)?,
                        artifact: rows::text(row, 2)?,
                        settings: row.get(3)?,
                        registration: optional_unsigned(row, 4)?,
                        based_on: optional_unsigned(row, 5)?,
                        status: row.get(6)?,
                        failure: row.get(7)?,
                    },
                ))
            },
        )?;
        let configurations = rows_of(
            connection,
            "SELECT id,registration,document,source,previous FROM config_generations ORDER BY id",
            |row| {
                Ok((
                    rows::unsigned(row, 0)?,
                    Configuration {
                        registration: rows::unsigned(row, 1)?,
                        document: row.get(2)?,
                        source: row.get(3)?,
                        previous: optional_unsigned(row, 4)?,
                    },
                ))
            },
        )?;
        let plugins = rows_of(
            connection,
            "SELECT id,named_by,active,active_by,rollback_target,rollback_by FROM plugins ORDER BY id",
            |row| {
                Ok(Plugin {
                    id: rows::text(row, 0)?,
                    named_by: optional_unsigned(row, 1)?,
                    active: (optional_unsigned(row, 2)?, optional_unsigned(row, 3)?),
                    rollback: (optional_unsigned(row, 4)?, optional_unsigned(row, 5)?),
                })
            },
        )?;
        let names = rows_of(
            connection,
            "SELECT registration,plugin_id,name,previous FROM registration_names
             ORDER BY registration,plugin_id",
            |row| {
                Ok(Name {
                    registration: rows::unsigned(row, 0)?,
                    plugin: rows::text(row, 1)?,
                    name: row.get(2)?,
                    previous: optional_unsigned(row, 3)?,
                })
            },
        )?;
        let code = rows_of(
            connection,
            "SELECT registration,plugin_id,generation,intent,source FROM registration_code
             ORDER BY registration,plugin_id",
            |row| {
                Ok(Code {
                    registration: rows::unsigned(row, 0)?,
                    plugin: rows::text(row, 1)?,
                    generation: rows::unsigned(row, 2)?,
                    intent: row.get(3)?,
                    source: row.get(4)?,
                })
            },
        )?;
        let config = rows_of(connection, "SELECT active FROM config", |row| {
            rows::unsigned(row, 0)
        })?
        .pop();
        let registry = Self {
            generations: generations.into_iter().collect(),
            configurations: configurations.into_iter().collect(),
            plugins,
            names,
            code,
            config,
        };
        registry.check_identities()?;
        Ok(registry)
    }

    /// A registration brought at most one generation per plugin, so the pair can identify it.
    fn check_identities(&self) -> Result<(), StoreError> {
        let mut seen = std::collections::BTreeSet::new();
        for generation in self.generations.values() {
            if let Some(registration) = generation.registration
                && !seen.insert((registration, generation.plugin))
            {
                return Err(backend(format!(
                    "registration {registration} brought two generations of plugin {}",
                    generation.plugin
                )));
            }
        }
        Ok(())
    }

    /// The code an old plugin generation number names: the generation a registration brought,
    /// or factory code by its artifact.
    pub fn code(&self, id: u64) -> Result<CodeRef, StoreError> {
        let generation = self.generations.get(&id).ok_or_else(|| unknown(id))?;
        Ok(CodeRef {
            plugin: generation.plugin,
            code: match generation.registration {
                Some(registration) => PluginCode::Generation {
                    registration: RegistrationId(registration),
                },
                None => PluginCode::Factory {
                    artifact: generation.artifact,
                },
            },
        })
    }

    /// The code an old generation number names, which must be `plugin`'s.
    pub fn plugin_code(&self, plugin: PluginId, id: u64) -> Result<PluginCode, StoreError> {
        let code = self.code(id)?;
        if code.plugin != plugin {
            return Err(backend(format!(
                "plugin {plugin} cannot run generation {id} of plugin {}",
                code.plugin
            )));
        }
        Ok(code.code)
    }

    /// What a registration activated for a plugin.
    fn activated(&self, registration: u64, plugin: PluginId) -> Result<&Code, StoreError> {
        self.code
            .iter()
            .find(|code| code.registration == registration && code.plugin == plugin)
            .ok_or_else(|| {
                backend(format!(
                    "registration {registration} activated no code for plugin {plugin}"
                ))
            })
    }

    /// A rolled-back trial: always a generation a registration brought.
    pub fn generation(&self, id: u64) -> Result<GenerationId, StoreError> {
        self.code(id)?
            .generation()
            .ok_or_else(|| backend(format!("generation {id} is factory code, never on trial")))
    }

    /// The configuration and plugin code that an old list of active generation numbers names.
    pub fn active(&self, ids: &[u64]) -> Result<ActiveCode, StoreError> {
        let mut configuration = None;
        let mut plugins = BTreeMap::new();
        for id in ids {
            if let Some(config) = self.configurations.get(id) {
                if configuration
                    .replace(RegistrationId(config.registration))
                    .is_some()
                {
                    return Err(backend("two active configurations"));
                }
                continue;
            }
            let code = self.code(*id)?;
            if plugins.insert(code.plugin, code.code).is_some() {
                return Err(backend(format!(
                    "two active generations of plugin {}",
                    code.plugin
                )));
            }
        }
        Ok(ActiveCode {
            configuration: configuration.ok_or_else(|| backend("no active configuration"))?,
            plugins,
        })
    }

    /// Write the rows into the version 24 tables, which must exist and be empty.
    pub fn write(&self, connection: &Connection) -> Result<(), StoreError> {
        for generation in self.generations.values() {
            let Some(registration) = generation.registration else {
                continue;
            };
            // Code a registration brought came from its code intent; settings, from the code they
            // reuse, with no intent.
            let activated = self.activated(registration, generation.plugin)?;
            let (intent, source, (based_on_generation, based_on_factory)) =
                match (generation.based_on, &activated.intent) {
                    (Some(based_on), None) => (
                        None,
                        None,
                        columns(self.plugin_code(generation.plugin, based_on)?)?,
                    ),
                    (None, Some(intent)) => (Some(intent), activated.source.as_ref(), (None, None)),
                    (Some(_), Some(_)) | (None, None) => {
                        return Err(backend(format!(
                            "registration {registration} records an intent that disagrees with \
                             where the code of plugin {} came from",
                            generation.plugin
                        )));
                    }
                };
            let (registration, plugin) = (signed(registration)?, generation.plugin.to_string());
            connection
                .execute(
                    "INSERT INTO plugin_generations(registration,plugin_id,artifact,settings,intent,
                         source,based_on_generation,based_on_factory)
                     VALUES (?,?,?,?,?,?,?,?)",
                    params![
                        registration,
                        plugin,
                        generation.artifact.to_string(),
                        generation.settings,
                        intent,
                        source,
                        based_on_generation,
                        based_on_factory
                    ],
                )
                .map_err(backend)?;
            connection
                .execute(
                    "INSERT INTO generation_health(registration,plugin_id,status,failure)
                     VALUES (?,?,?,?)",
                    params![registration, plugin, generation.status, generation.failure],
                )
                .map_err(backend)?;
        }
        self.write_names(connection)?;
        for code in &self.code {
            let factory = match self.plugin_code(code.plugin, code.generation)? {
                PluginCode::Generation { registration } if registration.0 == code.registration => {
                    None
                }
                PluginCode::Factory { artifact } if code.intent.as_deref() == Some("factory") => {
                    Some(artifact.to_string())
                }
                other => {
                    return Err(backend(format!(
                        "registration {} activated {other:?} with intent {:?}, which it did not \
                         bring or follow",
                        code.registration, code.intent
                    )));
                }
            };
            connection
                .execute(
                    "INSERT INTO registration_code(registration,plugin_id,factory) VALUES (?,?,?)",
                    params![signed(code.registration)?, code.plugin.to_string(), factory],
                )
                .map_err(backend)?;
        }
        self.write_configurations(connection)?;
        self.write_plugins(connection)
    }

    /// A version 23 reference: the code it names, and the registration that wrote it, which
    /// activated that code.
    fn reference(
        &self,
        plugin: PluginId,
        (generation, by): Reference,
    ) -> Result<Option<PluginActivation>, StoreError> {
        let Some(generation) = generation else {
            return match by {
                None => Ok(None),
                Some(by) => Err(backend(format!(
                    "registration {by} wrote a reference without code for plugin {plugin}"
                ))),
            };
        };
        let code = self.plugin_code(plugin, generation)?;
        let activation = match code {
            PluginCode::Generation { registration } if by == Some(registration.0) => {
                PluginActivation::Generation { registration }
            }
            PluginCode::Factory { artifact } => PluginActivation::Factory {
                artifact,
                registration: by.map(RegistrationId),
            },
            PluginCode::Generation { .. } => {
                return Err(backend(format!(
                    "plugin {plugin} runs {code}, which registration {by:?} did not bring"
                )));
            }
        };
        if let Some(by) = by
            && self.plugin_code(plugin, self.activated(by, plugin)?.generation)? != code
        {
            return Err(backend(format!(
                "registration {by} wrote a reference to {code} of plugin {plugin}, which it did not activate"
            )));
        }
        Ok(Some(activation))
    }

    fn write_names(&self, connection: &Connection) -> Result<(), StoreError> {
        for name in &self.names {
            let (registration, plugin) = (signed(name.registration)?, name.plugin.to_string());
            connection
                .execute(
                    "INSERT INTO registration_names(registration,plugin_id,name) VALUES (?,?,?)",
                    params![registration, plugin, name.name],
                )
                .map_err(backend)?;
            connection
                .execute(
                    "INSERT INTO replaced_names(registration,plugin_id,previous) VALUES (?,?,?)",
                    params![registration, plugin, name.previous.map(signed).transpose()?],
                )
                .map_err(backend)?;
        }
        Ok(())
    }

    fn write_configurations(&self, connection: &Connection) -> Result<(), StoreError> {
        let registration = |id: u64| -> Result<i64, StoreError> {
            let config = self
                .configurations
                .get(&id)
                .ok_or_else(|| backend(format!("configuration {id} is not recorded")))?;
            signed(config.registration)
        };
        for config in self.configurations.values() {
            connection
                .execute(
                    "INSERT INTO config_generations(registration,document,source) VALUES (?,?,?)",
                    params![signed(config.registration)?, config.document, config.source],
                )
                .map_err(backend)?;
            if let Some(previous) = config.previous {
                connection
                    .execute(
                        "INSERT INTO replaced_config(registration,previous) VALUES (?,?)",
                        params![signed(config.registration)?, registration(previous)?],
                    )
                    .map_err(backend)?;
            }
        }
        if let Some(active) = self.config {
            connection
                .execute(
                    "INSERT INTO config(singleton,active) VALUES (1,?)",
                    [registration(active)?],
                )
                .map_err(backend)?;
        }
        Ok(())
    }

    fn write_plugins(&self, connection: &Connection) -> Result<(), StoreError> {
        for plugin in &self.plugins {
            let (active_by, active_factory) = reference(self.reference(plugin.id, plugin.active)?)?;
            let (rollback_by, rollback_factory) =
                reference(self.reference(plugin.id, plugin.rollback)?)?;
            connection
                .execute(
                    "INSERT INTO plugins(id,named_by,active_by,active_factory,rollback_by,
                         rollback_factory)
                     VALUES (?,?,?,?,?,?)",
                    params![
                        plugin.id.to_string(),
                        plugin.named_by.map(signed).transpose()?,
                        active_by,
                        active_factory,
                        rollback_by,
                        rollback_factory
                    ],
                )
                .map_err(backend)?;
        }
        Ok(())
    }
}
