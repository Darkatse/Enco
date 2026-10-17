//! Restore persisted references; only first boot consumes a host-prepared registration.
use super::*;
use std::collections::BTreeSet;

pub(super) async fn open<E, F>(
    deps: &RegistryDeps,
    bootstrap: impl FnOnce(RegisteredIntent) -> F,
    registry_error: &impl Fn(RegistryError) -> E,
) -> Result<(State, ActiveConfig), E>
where
    F: std::future::Future<Output = Result<RegistrationRequest, E>>,
{
    let (mut state, current) = restore(deps).await.map_err(registry_error)?;
    let config = match current {
        Some(config) => config,
        None => {
            let request = bootstrap(
                state
                    .registered_intent(&deps.factory, None)
                    .map_err(registry_error)?,
            )
            .await?;
            register(deps, &mut state, request)
                .await
                .map_err(registry_error)?
        }
    };
    let references: Vec<_> = state.history.plugins.values().cloned().collect();
    factory::retain(deps, &state, &references)
        .await
        .map_err(registry_error)?;
    state.release();
    cleanup_artifacts(deps, &state).await;
    Ok((state, config))
}

async fn restore(deps: &RegistryDeps) -> Result<(State, Option<ActiveConfig>), RegistryError> {
    let mut state = State::new(deps.store.registry().await?);
    factory::record(deps, &mut state).await?;
    load_retained(deps, &mut state).await?;
    let current = state
        .history
        .active_config
        .map(|id| {
            let record = &state.history.config_generations[&id];
            configuration::declare(&record.document, &deps.owners)
                .map(|declaration| declaration.into_active(record))
                .map_err(RegistryError::Config)
        })
        .transpose()?;
    activation::restore(deps, &mut state).await?;
    Ok((state, current))
}

async fn register(
    deps: &RegistryDeps,
    state: &mut State,
    request: RegistrationRequest,
) -> Result<ActiveConfig, RegistryError> {
    let plan = registration::plan(deps, state, None, request).await?;
    let prepared = registration::prepare(deps, plan).await?;
    let (_, config, _) = registration::accept(deps, state, prepared).await?;
    config
        .ok_or_else(|| RegistryError::Bootstrap("initial registration has no configuration".into()))
}

async fn load_retained(deps: &RegistryDeps, state: &mut State) -> Result<(), RegistryError> {
    let ids: BTreeSet<_> = state
        .history
        .plugins
        .values()
        .flat_map(|plugin| [plugin.active, plugin.rollback_target])
        .flatten()
        .map(|reference| reference.generation)
        .collect();
    for id in ids {
        if state.loaded.contains_key(&id) {
            continue;
        }
        let record = state
            .history
            .generations
            .get(&id)
            .ok_or(RegistryError::UnknownGeneration(id))?;
        if let Ok(loaded) = activation::load(deps, record, None).await? {
            state.loaded.insert(id, loaded);
        }
    }
    Ok(())
}
