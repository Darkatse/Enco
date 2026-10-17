//! Development facts are sampled here; attention and presentation use the complete host status.
use super::workspace::PluginWorkspace;
use super::{Development, DevelopmentError, EntryError, Failed, Operation, WorkspaceComparison};
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
    pub registration: RegistrationId,
    /// Every plugin that runs, is registered, or has a directory.
    pub plugins: BTreeMap<PluginName, PluginUnit>,
    pub configuration: ConfigurationUnit,
    pub recent_registrations: Vec<RegistrationDetails>,
    /// Why the `plugins` directory cannot be listed; plugin workspace facts are then absent.
    pub plugins_unreadable: Option<EntryError>,
}

#[derive(Default, Serialize)]
pub struct PluginUnit {
    pub running: Option<PluginStatus>,
    pub registered: Option<RegisteredUnit>,
    pub workspace: Option<PluginWorkspace>,
}

#[derive(Serialize)]
pub struct RegisteredUnit {
    #[serde(flatten)]
    pub plugin: RegisteredPlugin,
    /// No build of this host's SDK produced the registered code; the next registration rebuilds it.
    pub earlier_sdk: bool,
    /// Why registering this host's factory code was rejected; every registration retries it.
    pub follow_rejection: Option<Vec<Cause>>,
}

#[derive(Serialize)]
pub struct ConfigurationUnit {
    pub running: ConfigurationRef,
    pub registered: ConfigurationRef,
    pub workspace: WorkspaceComparison,
    pub invalid: Vec<SettingsError>,
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
        let workspace = self.compare(&baseline.registered).await?;
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
        let plugins_unreadable = match workspace.plugins {
            Ok(directories) => {
                for (name, directory) in directories {
                    plugins.entry(name).or_default().workspace = Some(directory);
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
                workspace: workspace.configuration,
                invalid: baseline.config.invalid,
                unavailable_uses: baseline.config.unavailable_uses,
            },
            recent_registrations: baseline.recent_registrations,
            plugins_unreadable,
        })
    }
}
