use super::*;

/// The bounded registration history shown with current state.
pub const RECENT_REGISTRATIONS: usize = 5;

/// What runs for one plugin, and how that differs from its registered intent.
#[derive(serde::Serialize)]
pub struct PluginStatus {
    /// Immutable plugin identity.
    pub id: PluginId,
    /// Its registered name, or the name it runs under after a registration removed it.
    pub name: PluginName,
    /// What runs now.
    pub running: Running,
    /// Retained healthy generation, before the embedded factory fallback.
    pub rollback_target: Option<PluginActivation>,
    /// Summary of the selected component.
    pub summary: Option<String>,
    /// Configured users of the running name.
    pub users: Vec<String>,
    /// How what runs stands against the registered intent.
    pub standing: Standing,
}

/// The running layer of one plugin.
#[derive(serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Running {
    /// No generation runs under a name.
    Stopped,
    /// This host serves the selected generation.
    Serving {
        /// Routing name.
        name: PluginName,
        /// Selected generation and its provenance.
        activation: Activation,
    },
    /// A generation is selected, but this host cannot serve it.
    Unavailable {
        /// Routing name.
        name: PluginName,
        /// Selected generation and its provenance.
        activation: Activation,
        /// Why this host cannot serve it.
        causes: Vec<Cause>,
    },
}

impl Running {
    /// The routing name, absent when nothing runs.
    pub fn name(&self) -> Option<&PluginName> {
        match self {
            Self::Serving { name, .. } | Self::Unavailable { name, .. } => Some(name),
            Self::Stopped => None,
        }
    }

    /// The selected generation, whether or not this host can serve it.
    pub fn activation(&self) -> Option<&Activation> {
        match self {
            Self::Serving { activation, .. } | Self::Unavailable { activation, .. } => {
                Some(activation)
            }
            Self::Stopped => None,
        }
    }
}

/// How the running layer of one plugin stands against its registered layer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum Standing {
    /// No difference is listed when what runs agrees, or when nothing runs.
    Registered {
        /// The layers that differ.
        differences: Vec<Difference>,
    },
    /// A registration removed the plugin, and mechanical recovery still runs it.
    Removed {
        /// Registration that removed it.
        by: RegistrationId,
    },
}

/// One layer in which a registered plugin runs otherwise than registered.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Difference {
    /// The plugin runs under a name from an earlier registration.
    Name {
        /// Routing name.
        running: PluginName,
    },
    /// The running code differs from the registered code.
    Code,
    /// The running settings differ from the registered settings.
    Settings,
}

/// Plugin and configuration facts sampled under the same committer.
pub struct RegistryStatus {
    /// The same narrow accepted-intent query used to prepare registrations.
    pub registered: RegisteredIntent,
    /// Plugins that run or are registered.
    pub plugins: Vec<PluginStatus>,
    /// Current configuration and its validity.
    pub config: ConfigStatus,
    /// Recent registrations from the same locked observation as the current references.
    pub recent_registrations: Vec<RegistrationDetails>,
}

/// Configuration is one intent document; a rollback changes only its active reference.
pub struct ConfigStatus {
    /// Running configuration.
    pub running: ConfigurationRef,
    /// Configuration selected by the latest registration.
    pub registered: ConfigurationRef,
    /// Host tables which the current host cannot interpret.
    pub invalid: Vec<SettingsError>,
    /// Uses which cannot currently be served.
    pub unavailable_uses: Vec<UnservedUse>,
}

/// One current configured use that cannot be served by the selected exports.
#[derive(serde::Serialize)]
pub struct UnservedUse {
    /// Configured consumer, such as `memory.embedding`.
    pub user: String,
    /// Configuration entry named by that consumer.
    pub target: UseRef,
    /// Why the selected exports cannot serve it.
    pub causes: Vec<Cause>,
}

impl Registry {
    /// Inspect facts, never infer which sequence of recovery actions produced them.
    pub async fn status(&self) -> Result<RegistryStatus, RegistryError> {
        let state = self.state.lock().await;
        let view = self.view();
        let active = &state.history.config_generations[&view.config.generation];
        let registered = state.registered_intent(&self.deps.factory, None)?;
        let registered_config = &registered.accepted()?.configuration;
        let unavailable_uses: Vec<_> = view
            .config
            .uses
            .iter()
            .filter_map(|usage| {
                view.exports
                    .check_use(usage)
                    .err()
                    .map(|causes| UnservedUse {
                        user: usage.user.clone(),
                        target: usage.reference(),
                        causes,
                    })
            })
            .collect();
        let plugins = state
            .history
            .plugins
            .values()
            .filter(|plugin| {
                state.running_name(plugin.id).is_some()
                    || registered.plugin_by_id(plugin.id).is_some()
            })
            .map(|plugin| plugin_status(&state, &view, plugin, registered.plugin_by_id(plugin.id)))
            .collect::<Result<_, _>>()?;
        let recent_registrations =
            state.registration_history(None, RECENT_REGISTRATIONS, &self.deps.owners)?;
        let config = ConfigStatus {
            running: ConfigurationRef::of(active),
            registered: ConfigurationRef::of(registered_config),
            invalid: view.config.invalid.clone(),
            unavailable_uses,
        };
        drop(state);
        Ok(RegistryStatus {
            registered,
            plugins,
            config,
            recent_registrations: self.resolve_history(recent_registrations).await?,
        })
    }
}

fn plugin_status(
    state: &State,
    view: &View,
    plugin: &PluginRecord,
    registered: Option<(&PluginName, &RegisteredPlugin)>,
) -> Result<PluginStatus, RegistryError> {
    let running = match state.running_name(plugin.id) {
        None => Running::Stopped,
        Some(name) => {
            let reference = plugin.active.ok_or_else(|| {
                RegistryError::Intent(format!("running plugin {name} has no active generation"))
            })?;
            let activation = state.activation(reference)?;
            match view.exports.loaded(name) {
                Some(Ok(_)) => Running::Serving {
                    name: name.clone(),
                    activation,
                },
                Some(Err(causes)) => Running::Unavailable {
                    name: name.clone(),
                    activation,
                    causes: causes.to_vec(),
                },
                None => {
                    return Err(RegistryError::Intent(format!(
                        "running plugin {name} has no published route"
                    )));
                }
            }
        }
    };
    let name = registered
        .map(|(name, _)| name)
        .or_else(|| running.name())
        .cloned()
        .ok_or_else(|| {
            RegistryError::Intent(format!(
                "plugin {} neither runs nor is registered",
                plugin.id
            ))
        })?;
    let standing = match registered {
        Some((registered_name, registered)) => Standing::Registered {
            differences: match &running {
                Running::Serving { name, activation }
                | Running::Unavailable {
                    name, activation, ..
                } => differences(name, activation, registered_name, &registered.activation),
                Running::Stopped => Vec::new(),
            },
        },
        None => {
            let (by, _) = state
                .registered_name(plugin.id, None)
                .filter(|(_, name)| name.name.is_none())
                .ok_or_else(|| {
                    RegistryError::Intent(format!(
                        "running plugin {} is neither registered nor removed",
                        plugin.id
                    ))
                })?;
            Standing::Removed { by }
        }
    };
    Ok(PluginStatus {
        id: plugin.id,
        name,
        summary: running
            .activation()
            .and_then(|activation| state.loaded.get(&activation.generation.id))
            .map(|loaded| loaded.summary.clone()),
        users: running
            .name()
            .map_or_else(Vec::new, |name| users(&view.config.uses, name)),
        rollback_target: plugin.rollback_target,
        standing,
        running,
    })
}

fn differences(
    running_name: &PluginName,
    running: &Activation,
    registered_name: &PluginName,
    registered: &Activation,
) -> Vec<Difference> {
    let mut differences = Vec::new();
    if running_name != registered_name {
        differences.push(Difference::Name {
            running: running_name.clone(),
        });
    }
    if running.generation.artifact != registered.generation.artifact {
        differences.push(Difference::Code);
    }
    if running.generation.settings != registered.generation.settings {
        differences.push(Difference::Settings);
    }
    differences
}
