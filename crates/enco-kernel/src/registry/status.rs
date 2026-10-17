use super::*;

/// Identity, content and routing for one installation, including removed identities.
#[derive(serde::Serialize)]
pub struct PluginStatus {
    /// Immutable installation identity.
    pub id: PluginId,
    /// Current routing name; absent while not running.
    pub name: Option<String>,
    /// Concrete name version currently referenced by this identity.
    pub named_by: Option<RegistrationId>,
    /// Current generation, if running.
    pub active: Option<GenerationRecord>,
    /// Registration which wrote the current code reference, independently of its creation.
    pub activated_by: Option<RegistrationId>,
    /// Generation which supplied the active code; settings-only generations reuse this provenance.
    pub active_code: Option<GenerationId>,
    /// Retained healthy generation, before the embedded factory fallback.
    pub rollback_target: Option<PluginActivation>,
    /// Current embedded component, when this is a host-reserved plugin.
    pub factory: Option<FactoryStatus>,
    /// Whether name, code and settings agree with accepted intent.
    pub matches_registered: bool,
    /// Summary of the active component.
    pub summary: Option<String>,
    /// Configured users of the current name.
    pub users: Vec<String>,
    /// Reason the current component cannot be used.
    pub unavailable: Option<String>,
    /// Each configured use that cannot be served.
    pub unavailable_uses: Vec<String>,
    /// Complete generation history in commit order.
    pub generations: Vec<GenerationRecord>,
}

/// Plugin and configuration facts sampled under the same committer.
#[derive(serde::Serialize)]
pub struct RegistryStatus {
    /// The same narrow accepted-intent query used to prepare registrations.
    pub registered: RegisteredIntent,
    /// Accepted changes, including names and removals.
    pub registrations: Vec<RegistrationRecord>,
    /// Every installation's content and references.
    pub plugins: Vec<PluginStatus>,
    /// Current configuration and its history.
    pub config: ConfigStatus,
}

/// Configuration is one intent document; a rollback changes only its active reference.
#[derive(serde::Serialize)]
pub struct ConfigStatus {
    /// Current configuration document and provenance.
    pub active: ConfigGeneration,
    /// Complete configuration history.
    pub generations: Vec<ConfigGeneration>,
    /// Host tables which the current host cannot interpret.
    pub invalid: Vec<SettingsError>,
    /// Uses which cannot currently be served.
    pub unavailable_uses: Vec<String>,
}

impl Registry {
    /// Inspect facts, never infer which sequence of recovery actions produced them.
    pub async fn status(&self) -> Result<RegistryStatus, RegistryError> {
        let state = self.state.lock().await;
        let view = self.view();
        let active = &state.history.config_generations[&view.config.generation];
        let registered = state.registered_intent(&self.deps.factory);
        let registered_settings: BTreeMap<String, PluginSettings> = registered
            .configuration
            .as_ref()
            .and_then(|config| config.document.get("plugin"))
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|error| rejected("plugin", error))?
            .unwrap_or_default();
        let unavailable_uses: Vec<_> = view
            .config
            .uses
            .iter()
            .filter_map(|usage| {
                view.exports
                    .check_use(usage)
                    .err()
                    .map(|failure| (usage, format!("{}: {}", usage.user, failure.message)))
            })
            .collect();
        let plugins = state
            .history
            .plugins
            .values()
            .map(|plugin| {
                let accepted = registered
                    .plugins
                    .iter()
                    .find(|(_, accepted)| accepted.id == plugin.id);
                let settings = accepted
                    .and_then(|(name, _)| registered_settings.get(name))
                    .cloned()
                    .unwrap_or_default();
                plugin_status(
                    &state,
                    &view,
                    plugin,
                    accepted,
                    &settings,
                    &unavailable_uses,
                )
            })
            .collect();
        Ok(RegistryStatus {
            registered,
            registrations: state.history.registrations.values().cloned().collect(),
            plugins,
            config: ConfigStatus {
                active: active.clone(),
                generations: state.history.config_generations.values().cloned().collect(),
                invalid: view.config.invalid.clone(),
                unavailable_uses: unavailable_uses
                    .into_iter()
                    .map(|(_, reason)| reason)
                    .collect(),
            },
        })
    }
}

fn plugin_status(
    state: &State,
    view: &View,
    plugin: &PluginRecord,
    registered: Option<(&String, &RegisteredPlugin)>,
    settings: &PluginSettings,
    unavailable_uses: &[(&Use, String)],
) -> PluginStatus {
    let name = state.running_name(plugin.id).map(str::to_owned);
    let active = state.active(plugin.id).cloned();
    let matches_registered = name.as_ref() == registered.map(|(name, _)| name)
        && match (
            &active,
            registered.and_then(|(_, plugin)| plugin.code.as_ref()),
        ) {
            (Some(active), Some(code)) => {
                active.artifact == code.artifact && &active.settings == settings
            }
            (None, None) => true,
            _ => false,
        };
    PluginStatus {
        id: plugin.id,
        name: name.clone(),
        named_by: plugin.named_by,
        summary: active
            .as_ref()
            .and_then(|record| state.loaded.get(&record.id))
            .map(|loaded| loaded.summary.clone()),
        active_code: active
            .as_ref()
            .map(|active| state.code_generation(active.id)),
        activated_by: plugin.active.and_then(|reference| reference.registration),
        active,
        rollback_target: plugin.rollback_target,
        factory: registered.and_then(|(_, plugin)| plugin.factory.clone()),
        matches_registered,
        users: name
            .as_ref()
            .map_or_else(Vec::new, |name| users(&view.config.uses, name)),
        unavailable: name
            .as_ref()
            .and_then(|name| view.exports.loaded(name).err())
            .map(|failure| failure.message),
        unavailable_uses: unavailable_uses
            .iter()
            .filter(|(usage, _)| Some(&usage.endpoint.plugin) == name.as_ref())
            .map(|(_, reason)| reason.clone())
            .collect(),
        generations: state
            .history
            .generations
            .values()
            .filter(|record| record.plugin == plugin.id)
            .cloned()
            .collect(),
    }
}
