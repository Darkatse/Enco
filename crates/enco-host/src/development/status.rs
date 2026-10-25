//! The three layers of the home, sampled once: what runs, what is registered, and what the home
//! holds.
//!
//! Deciding what needs attention and presenting it use the complete host status, in `status`.
use super::{
    Development, DevelopmentError, EntryError, Failed, HomeComparison, HomePlugin, Operation,
};
use enco_core::{ConfigurationRef, PluginName, RegistrationId};
use enco_kernel::{
    Cause, PluginStatus, RegisteredPlugin, RegistrationDetails, Registry, SettingsError,
    UnservedUse,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// The three layers of every plugin and of `config.toml`, observed once.
#[derive(Serialize)]
pub struct DevelopmentStatus {
    /// The latest registration, which the registered layer reflects.
    pub registration: RegistrationId,
    /// Every plugin that runs, is registered, or has a directory.
    pub plugins: BTreeMap<PluginName, PluginUnit>,
    /// The three layers of `config.toml`.
    pub configuration: ConfigurationUnit,
    /// The latest registrations, newest first.
    pub recent_registrations: Vec<RegistrationDetails>,
    /// Why the `plugins` directory cannot be listed; the home facts of plugins are then absent.
    pub plugins_unreadable: Option<EntryError>,
}

/// The three layers of one plugin; a layer is absent when the plugin is not in it.
#[derive(Default, Serialize)]
pub struct PluginUnit {
    /// What runs under this name.
    pub running: Option<PluginStatus>,
    /// What the latest registration holds for this name.
    pub registered: Option<RegisteredUnit>,
    /// How the plugin's directory in the home compares with what is registered.
    pub home: Option<HomePlugin>,
}

/// What the latest registration holds for one plugin.
#[derive(Serialize)]
pub struct RegisteredUnit {
    /// The registered name, identity and code choice.
    #[serde(flatten)]
    pub plugin: RegisteredPlugin,
    /// No build of this host's SDK produced the registered code; the next registration rebuilds it.
    pub earlier_sdk: bool,
    /// Why registering this host's factory code was rejected; every registration retries it.
    pub follow_rejection: Option<Vec<Cause>>,
}

/// The three layers of `config.toml`.
#[derive(Serialize)]
pub struct ConfigurationUnit {
    /// The configuration in use; it differs from the registered one after a rollback.
    pub running: ConfigurationRef,
    /// The configuration the latest registration selected.
    pub registered: ConfigurationRef,
    /// How `config.toml` in the home compares with the registered configuration.
    pub home: HomeComparison,
    /// Tables in the running configuration that this host cannot interpret.
    pub invalid: Vec<SettingsError>,
    /// Configured uses that no running plugin can serve now.
    pub unavailable_uses: Vec<UnservedUse>,
}

impl Development {
    /// Inspect source and registry facts without building or changing either of them.
    pub async fn status(&self, registry: &Registry) -> Result<DevelopmentStatus, Failed> {
        self.observe(registry)
            .await
            .map_err(|error| Operation::Status.failed(error))
    }

    async fn observe(&self, registry: &Registry) -> Result<DevelopmentStatus, DevelopmentError> {
        let baseline = registry.status().await?;
        let registration = baseline.registered.accepted()?.id;
        let home = self.compare(&baseline.registered).await?;
        let follow = self.follow_rejections.read().await;
        let mut plugins: BTreeMap<PluginName, PluginUnit> = BTreeMap::new();
        for running in baseline.plugins {
            let name = running.name.clone();
            plugins.entry(name).or_default().running = Some(running);
        }
        for (name, plugin) in baseline.registered.plugins {
            let earlier_sdk = match &plugin.code {
                Some(code) => self.built_with_earlier_sdk(code).await?,
                None => false,
            };
            plugins.entry(name).or_default().registered = Some(RegisteredUnit {
                follow_rejection: follow.get(&plugin.id).cloned(),
                earlier_sdk,
                plugin,
            });
        }
        let plugins_unreadable = match home.plugins {
            Ok(directories) => {
                for (name, directory) in directories {
                    plugins.entry(name).or_default().home = Some(directory);
                }
                None
            }
            Err(error) => Some(error),
        };
        Ok(DevelopmentStatus {
            registration,
            plugins,
            configuration: ConfigurationUnit {
                running: baseline.config.running,
                registered: baseline.config.registered,
                home: home.configuration,
                invalid: baseline.config.invalid,
                unavailable_uses: baseline.config.unavailable_uses,
            },
            recent_registrations: baseline.recent_registrations,
            plugins_unreadable,
        })
    }
}
