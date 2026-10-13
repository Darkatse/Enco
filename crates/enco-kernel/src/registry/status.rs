use super::*;

/// Inspectable identity, history and routing for one registered name.
#[derive(serde::Serialize)]
pub struct PluginStatus {
    /// Human-facing name.
    pub name: String,
    /// Stable identity from plugins.lock.
    pub id: PluginId,
    /// Current generation, if available.
    pub active: Option<GenerationRecord>,
    /// Description of the active component, if loaded.
    pub summary: Option<String>,
    /// First recovery candidate, including the current factory fallback.
    pub rollback_target: Option<GenerationId>,
    /// Configured users of this plugin.
    pub users: Vec<String>,
    /// Configured users whose requirements the active generation cannot satisfy.
    pub unavailable: Vec<String>,
    /// Complete history in registry sequence order.
    pub generations: Vec<GenerationRecord>,
}

/// Plugin and configuration histories from the same committer snapshot.
#[derive(serde::Serialize)]
pub struct RegistryStatus {
    /// Registered plugin identities and histories.
    pub plugins: Vec<PluginStatus>,
    /// Active configuration and its history.
    pub config: ConfigStatus,
}

/// Configuration history and current host interpretation failures.
#[derive(serde::Serialize)]
pub struct ConfigStatus {
    /// Active generation.
    pub active: GenerationId,
    /// Earlier configuration selected by mechanical rollback.
    pub rollback_target: Option<GenerationId>,
    /// Generation numbers and observation times in commit order.
    pub generations: Vec<(GenerationId, DateTime<Utc>)>,
    /// Tables the current host cannot interpret.
    pub invalid: Vec<SettingsError>,
}

impl Registry {
    /// Read the registry's single owned state without maintaining a second status cache.
    pub async fn status(&self) -> RegistryStatus {
        let state = self.state.lock().await;
        let plugins = state
            .plugins
            .names
            .iter()
            .map(|(name, id)| {
                let active = state
                    .plugins
                    .active
                    .get(id)
                    .copied()
                    .and_then(|id| state.plugins.generations.get(&id));
                PluginStatus {
                    name: name.clone(),
                    id: *id,
                    summary: active
                        .and_then(|record| state.plugins.loaded.get(&record.id))
                        .map(|loaded| loaded.summary.clone()),
                    rollback_target: active.and_then(|record| {
                        state
                            .plugins
                            .candidates(*id, record.id)
                            .first()
                            .map(|record| record.id)
                    }),
                    active: active.cloned(),
                    users: users(&state.config.uses, name),
                    unavailable: state
                        .config
                        .uses
                        .iter()
                        .filter(|usage| usage.endpoint.plugin == *name)
                        .filter_map(|usage| {
                            match active.and_then(|record| state.plugins.loaded.get(&record.id)) {
                                Some(loaded) => wiring_error(usage, loaded),
                                None => Some(format!("{}: no active generation", usage.user)),
                            }
                        })
                        .collect(),
                    generations: state
                        .plugins
                        .generations
                        .values()
                        .filter(|record| record.plugin == *id)
                        .cloned()
                        .collect(),
                }
            })
            .collect();
        RegistryStatus {
            plugins,
            config: ConfigStatus {
                active: state.config.generation,
                rollback_target: state
                    .configs
                    .range(..state.config.generation)
                    .next_back()
                    .map(|(id, _)| *id),
                generations: state
                    .configs
                    .values()
                    .map(|record| (record.id, record.created_at))
                    .collect(),
                invalid: state.config.invalid.clone(),
            },
        }
    }
}
