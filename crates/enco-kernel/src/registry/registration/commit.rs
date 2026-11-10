use super::prepare::{Action, Prepared};
use super::*;
use crate::Replaced;

/// The complete write, with identities derived from the registration.
struct Identified {
    write: RegistrationWrite,
    loaded: BTreeMap<CodeRef, Loaded>,
}

pub(in crate::registry) async fn accept(
    deps: &RegistryDeps,
    state: &mut State,
    prepared: Prepared,
) -> Result<(Registered, Option<ActiveConfig>, admission::Admission), RegistryError> {
    if !prepared.plan.expected.matches(state) {
        return Err(RegistryError::Conflict("registration".into()));
    }
    if prepared.plan.configuration.is_some() {
        configuration::check_profiles(deps, &prepared.plan.declaration.kernel).await?;
    }
    let identified = Identified::new(deps, state, &prepared);
    let before = admission::evaluate(state.contents(), &deps.host_tool_names);
    let after = admission::evaluate(contents(state, &identified)?, &deps.host_tool_names);
    let names: Vec<_> = prepared
        .plan
        .plugins
        .values()
        .filter_map(|action| action.name().cloned())
        .collect();
    after.accept_change(
        &before,
        &names,
        &prepared.plan.declaration.uses,
        &prepared.plan.previous_uses,
    )?;
    let changes = state.registration_changes(
        &identified.write.registration,
        &identified.write.generations,
        identified.write.configuration.as_ref(),
        &deps.owners,
    )?;
    persist(deps, state, &identified.write, &prepared).await?;
    let config = identified
        .write
        .configuration
        .as_ref()
        .map(|record| prepared.plan.declaration.into_active(record));
    let users = config
        .as_ref()
        .map_or_else(
            || prepared.plan.previous_uses.iter(),
            |config| config.uses.iter(),
        )
        .filter(|usage| names.contains(usage.plugin()))
        .map(|usage| usage.user.clone())
        .collect();
    let result = Registered {
        registration: identified.write.registration.clone(),
        changes,
        generations: identified.write.generations.clone(),
        configuration: identified.write.configuration.clone(),
        probes: prepared.probes,
        users,
        warnings: Vec::new(),
    };
    state.registered(identified.write, identified.loaded);
    Ok((result, config, after))
}

impl Identified {
    fn new(deps: &RegistryDeps, state: &State, prepared: &Prepared) -> Self {
        let registration = RegistrationRecord {
            id: prepared.plan.expected.next,
            initiator: prepared.plan.initiator.clone(),
            names: BTreeMap::new(),
            code: BTreeMap::new(),
            created_at: deps.clock.now(),
        };
        let mut identified = Self {
            write: RegistrationWrite {
                registration,
                generations: Vec::new(),
                configuration: None,
                plugins: Vec::new(),
                replaced: Replaced::default(),
            },
            loaded: BTreeMap::new(),
        };
        for (id, action) in &prepared.plan.plugins {
            let mut plugin = state
                .node
                .plugins
                .get(id)
                .cloned()
                .unwrap_or_else(|| empty_plugin(*id));
            let previous = plugin.named_by;
            let activated = identified.update_plugin(state, &mut plugin, action);
            if plugin.named_by == Some(identified.write.registration.id) {
                identified.write.registration.names.insert(
                    *id,
                    RegisteredName {
                        name: action.name().cloned(),
                    },
                );
                identified.write.replaced.names.insert(*id, previous);
            }
            if let Some(code) = activated {
                identified.write.registration.code.insert(*id, code);
            }
            identified.write.plugins.push(plugin);
        }
        if let Some(config) = &prepared.plan.configuration {
            identified.write.configuration = Some(ConfigGeneration {
                registration: identified.write.registration.id,
                document: config.document.clone(),
                source: config.source,
            });
            identified.write.replaced.configuration = state.node.configuration;
        }
        identified
    }

    /// Apply one action to the plugin's references; the code it activates, if any.
    ///
    /// A registration brings at most one generation per plugin, so the pair is its identity.
    fn update_plugin(
        &mut self,
        state: &State,
        plugin: &mut PluginRecord,
        action: &Action<Box<Loaded>>,
    ) -> Option<RegisteredCode> {
        let registration = self.write.registration.id;
        let (code, loaded) = match action {
            Action::Code {
                content, loaded, ..
            } => {
                self.write.generations.push(GenerationRecord {
                    id: GenerationId {
                        registration,
                        plugin: plugin.id,
                    },
                    artifact: ContentHash::of(&content.bytes),
                    settings: content.settings.clone(),
                    origin: content.origin.clone(),
                });
                (RegisteredCode::Generation, loaded)
            }
            Action::Factory {
                artifact, loaded, ..
            } => (
                RegisteredCode::Factory {
                    artifact: *artifact,
                },
                loaded,
            ),
            Action::Remove => {
                plugin.rollback_target = next_rollback(state, plugin);
                plugin.active = None;
                plugin.named_by = Some(registration);
                return None;
            }
            Action::Rename { .. } => {
                plugin.named_by = Some(registration);
                return None;
            }
        };
        self.loaded.insert(
            CodeRef {
                plugin: plugin.id,
                code: code.code(registration),
            },
            loaded.as_ref().clone(),
        );
        plugin.rollback_target = next_rollback(state, plugin);
        if plugin.active.is_none() {
            plugin.named_by = Some(registration);
        }
        plugin.active = Some(PluginActivation::registered(registration, code));
        Some(code)
    }
}

/// Evaluate the complete result, including removals and renames, without changing live routes.
fn contents(
    state: &State,
    identified: &Identified,
) -> Result<BTreeMap<PluginName, Option<Loaded>>, RegistryError> {
    let mut identities = state.node.plugins.clone();
    identities.extend(
        identified
            .write
            .plugins
            .iter()
            .cloned()
            .map(|plugin| (plugin.id, plugin)),
    );
    let mut contents = BTreeMap::new();
    for plugin in identities.values() {
        let Some(active) = &plugin.active else {
            continue;
        };
        let name = if plugin.named_by == Some(identified.write.registration.id) {
            identified.write.registration.names.get(&plugin.id)
        } else {
            state.name_at(plugin.id, plugin.named_by)
        }
        .and_then(|record| record.name.as_ref());
        let Some(name) = name else {
            return Err(RegistryError::Intent(format!(
                "active plugin {} has no routing name",
                plugin.id
            )));
        };
        let code = CodeRef {
            plugin: plugin.id,
            code: active.code(),
        };
        let loaded = identified
            .loaded
            .get(&code)
            .or(state.loaded.get(&code))
            .cloned();
        if contents.insert(name.clone(), loaded).is_some() {
            return Err(rejected(Cause::NameTaken { name: name.clone() }));
        }
    }
    Ok(contents)
}

/// Publish references only after their files exist; rejected writes release their staged files.
async fn persist(
    deps: &RegistryDeps,
    state: &State,
    write: &RegistrationWrite,
    prepared: &Prepared,
) -> Result<(), RegistryError> {
    let result: Result<(), RegistryError> = async {
        factory::retain(deps, &write.plugins).await?;
        for action in prepared.plan.plugins.values() {
            if let Action::Code { content, .. } = action {
                deps.store.put_artifact(&content.bytes).await?;
            }
        }
        deps.store.register(write).await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        return Err(cleanup_failed_write(deps, state, error).await);
    }
    Ok(())
}
