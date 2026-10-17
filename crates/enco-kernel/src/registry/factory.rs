//! Embedded generations are host content; ordinary registration alone activates factory intent.
use super::*;

pub(super) async fn record(deps: &RegistryDeps, state: &mut State) -> Result<(), RegistryError> {
    let names = state.registered_names();
    let running = state.running_names();
    let mut next = state.next_generation();
    let mut records = Vec::new();
    for factory in &deps.factory {
        if names
            .get(&factory.name)
            .or(running.get(&factory.name))
            .is_some_and(|id| *id != factory.id)
        {
            return Err(RegistryError::FactoryIdentity(factory.name.clone()));
        }
        let hash = ContentHash::of(&factory.artifact);
        let existing = state.history.generations.values().rev().find(|record| {
            record.plugin == factory.id
                && record.artifact == hash
                && record.origin == Origin::Factory
                && record.status == GenerationStatus::Healthy
        });
        let generation = match existing {
            Some(record) => record.clone(),
            None => {
                let record = GenerationRecord {
                    id: next,
                    plugin: factory.id,
                    artifact: hash,
                    settings: PluginSettings::default(),
                    origin: Origin::Factory,
                    status: GenerationStatus::Healthy,
                    failure: None,
                    created_at: deps.clock.now(),
                };
                next.0 += 1;
                records.push(record.clone());
                record
            }
        };
        let loaded = deps
            .runtime
            .load(&factory.artifact, &generation.settings)
            .await
            .map_err(|error| {
                rejected(Cause::Load {
                    plugin: factory.name.clone(),
                    candidate: Proposed::Factory,
                    error,
                })
            })?;
        state.factory.insert(factory.id, generation.id);
        state.loaded.insert(generation.id, loaded);
    }
    if !records.is_empty() {
        deps.store.record_factories(&records).await?;
    }
    for record in records {
        state
            .history
            .plugins
            .entry(record.plugin)
            .or_insert(PluginRecord {
                id: record.plugin,
                named_by: None,
                active: None,
                rollback_target: None,
            });
        state.history.generations.insert(record.id, record);
    }
    Ok(())
}

/// An activated factory must survive a later binary upgrade while its reference is retained.
pub(super) async fn retain(
    deps: &RegistryDeps,
    state: &State,
    plugins: &[PluginRecord],
) -> Result<(), RegistryError> {
    for factory in &deps.factory {
        let Some(generation) = state.factory.get(&factory.id) else {
            continue;
        };
        let hash = state.history.generations[generation].artifact;
        let referenced = plugins
            .iter()
            .flat_map(|plugin| [plugin.active, plugin.rollback_target])
            .flatten()
            .filter_map(|reference| state.history.generations.get(&reference.generation))
            .any(|record| record.artifact == hash);
        if referenced {
            deps.store.put_artifact(&factory.artifact).await?;
        }
    }
    Ok(())
}
