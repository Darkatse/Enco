//! Factory code is host content, identified by its bytes; only an ordinary registration
//! activates factory intent.
use super::*;

/// Load this host's factory code; nothing is recorded, because every node derives the same
/// identity from the same bytes.
pub(super) async fn load(deps: &RegistryDeps, state: &mut State) -> Result<(), RegistryError> {
    let names = state.registered_names();
    let running = state.running_names();
    for factory in &deps.factory {
        if names
            .get(&factory.name)
            .or(running.get(&factory.name))
            .is_some_and(|id| *id != factory.id)
        {
            return Err(RegistryError::FactoryIdentity(factory.name.clone()));
        }
        let loaded = deps
            .runtime
            .load(factory.artifact, &PluginSettings::default())
            .await
            .map_err(|error| {
                rejected(Cause::Load {
                    plugin: factory.name.clone(),
                    candidate: Proposed::Factory,
                    error,
                })
            })?;
        let artifact = ContentHash::of(factory.artifact);
        state.factory.insert(factory.id, artifact);
        state.loaded.insert(
            CodeRef {
                plugin: factory.id,
                code: PluginCode::Factory { artifact },
            },
            loaded,
        );
    }
    Ok(())
}

/// Embedded bytes a reference names as factory code are written as a file for a later host.
///
/// A generation's bytes were written when it was registered.
pub(super) async fn retain(
    deps: &RegistryDeps,
    plugins: &[PluginRecord],
) -> Result<(), RegistryError> {
    for factory in &deps.factory {
        let hash = ContentHash::of(factory.artifact);
        let referenced = plugins
            .iter()
            .flat_map(|plugin| [plugin.active, plugin.rollback_target])
            .flatten()
            .any(|reference| reference.code() == PluginCode::Factory { artifact: hash });
        if referenced {
            deps.store.put_artifact(factory.artifact).await?;
        }
    }
    Ok(())
}
