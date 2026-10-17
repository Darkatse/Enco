//! One preparation and commit path for source, configuration, factory code and direct artifacts.
use super::*;
use crate::{RegistrationWrite, RegistryState};
mod commit;
pub(super) use commit::accept;
mod prepare;
pub(super) use prepare::{Prepared, artifact, plan, prepare};

/// Expectations contain only references, not a second copy of immutable history.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Expected {
    next: RegistrationId,
    config: Option<GenerationId>,
    plugins: BTreeMap<PluginId, PluginRecord>,
}

impl Expected {
    pub fn capture(state: &State) -> Self {
        Self {
            next: state.next_registration(),
            config: state.history.active_config,
            plugins: state.history.plugins.clone(),
        }
    }
    pub fn matches(&self, state: &State) -> bool {
        *self == Self::capture(state)
    }
}

impl Registry {
    /// Check the same rename candidate before the host moves its source directory.
    pub async fn check_rename(&self, name: &str, to: &str) -> Result<(), RegistryError> {
        let state = self.state.lock().await;
        let view = self.view();
        let identities = state.registered_identities(&self.deps.factory);
        prepare::check_rename(
            &self.deps,
            &state,
            &identities,
            Some(&view.config),
            name,
            to,
        )
    }

    /// Validate outside the committer, then accept every part against one current composition.
    pub async fn register(
        &self,
        request: RegistrationRequest,
    ) -> Result<Registered, RegistryError> {
        let plan = {
            let state = self.state.lock().await;
            let view = self.view();
            plan(&self.deps, &state, Some(&view.config), request).await?
        };
        let mut prepared = prepare(&self.deps, plan).await?;
        self.probe_registration(&mut prepared).await?;
        let mut state = self.state.lock().await;
        let (mut result, config, admission) =
            commit::accept(&self.deps, &mut state, prepared).await?;
        self.publish(&mut state, config, Some(admission));
        result
            .warnings
            .extend(cleanup_artifacts(&self.deps, &state).await);
        Ok(result)
    }

    async fn probe_registration(&self, prepared: &mut Prepared) -> Result<(), RegistryError> {
        if prepared.plan.configuration.is_none() {
            return Ok(());
        }
        let mut checked: Vec<Use> = Vec::new();
        let view = self.view();
        for usage in &prepared.plan.declaration.uses {
            if prepared
                .plan
                .previous_uses
                .iter()
                .chain(checked.iter())
                .any(|previous| previous.same_call(usage))
            {
                continue;
            }
            let (loaded, generation) = match prepared.loaded_for(&usage.endpoint.plugin) {
                Some(loaded) => (loaded, None),
                None => {
                    let (generation, loaded) = view
                        .exports
                        .loaded(&usage.endpoint.plugin)
                        .map_err(|failure| rejected(&usage.endpoint.plugin, failure.message))?;
                    (loaded, Some(generation))
                }
            };
            let outcome = match probe::call(loaded, usage).await {
                Ok(result) => {
                    if let Some(generation) = generation {
                        self.report(generation, result.verdict, None).await?;
                    }
                    result.outcome
                }
                Err(failure) => Err(failure),
            };
            outcome.map_err(|failure| {
                rejected(
                    "config",
                    format!(
                        "{} (endpoint {}): {}: {}",
                        usage.user, usage.endpoint.name, failure.code, failure.message
                    ),
                )
            })?;
            prepared.probes.push(Probe {
                user: usage.user.clone(),
                endpoint: usage.endpoint.name.clone(),
            });
            checked.push(usage.clone());
        }
        Ok(())
    }
}

fn settings(
    document: &serde_json::Value,
) -> Result<BTreeMap<String, PluginSettings>, RegistryError> {
    document.get("plugin").map_or_else(
        || Ok(BTreeMap::new()),
        |value| serde_json::from_value(value.clone()).map_err(|error| rejected("plugin", error)),
    )
}

fn empty_plugin(id: PluginId) -> PluginRecord {
    PluginRecord {
        id,
        named_by: None,
        active: None,
        rollback_target: None,
    }
}

fn next_rollback(history: &RegistryState, plugin: &PluginRecord) -> Option<PluginActivation> {
    match plugin
        .active
        .and_then(|active| history.generations.get(&active.generation))
    {
        Some(record) if record.status == GenerationStatus::Healthy => plugin.active,
        _ => plugin.rollback_target,
    }
}
