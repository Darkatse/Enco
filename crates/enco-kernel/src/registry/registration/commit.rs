use super::prepare::{Action, Content, Prepared};
use super::*;

struct Numbered {
    write: RegistrationWrite,
    loaded: BTreeMap<GenerationId, Loaded>,
    next: GenerationId,
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
    let numbered = Numbered::new(deps, state, &prepared)?;
    let before = admission::evaluate(state.contents(), &deps.host_tool_names);
    let after = admission::evaluate(contents(state, &numbered)?, &deps.host_tool_names);
    let names: Vec<_> = prepared
        .plan
        .plugins
        .values()
        .filter_map(|plugin| plugin.name.clone())
        .collect();
    after.accept_change(
        &before,
        &names,
        &prepared.plan.declaration.uses,
        &prepared.plan.previous_uses,
    )?;
    let changes = state.registration_changes(
        &numbered.write.registration,
        &numbered.write.generations,
        numbered.write.configuration.as_ref(),
        &deps.owners,
    )?;
    persist(deps, state, &numbered.write, &prepared).await?;
    let config = numbered
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
        registration: numbered.write.registration.clone(),
        changes,
        generations: numbered.write.generations.clone(),
        configuration: numbered.write.configuration.clone(),
        probes: prepared.probes,
        users,
        warnings: Vec::new(),
    };
    state.registered(numbered.write, numbered.loaded);
    Ok((result, config, after))
}

impl Numbered {
    fn new(deps: &RegistryDeps, state: &State, prepared: &Prepared) -> Result<Self, RegistryError> {
        let registration = RegistrationRecord {
            id: prepared.plan.expected.next,
            initiator: prepared.plan.initiator.clone(),
            names: BTreeMap::new(),
            code: BTreeMap::new(),
            created_at: deps.clock.now(),
        };
        let mut numbered = Self {
            write: RegistrationWrite {
                registration,
                generations: Vec::new(),
                configuration: None,
                plugins: Vec::new(),
            },
            loaded: BTreeMap::new(),
            next: state.next_generation(),
        };
        for (id, pending) in &prepared.plan.plugins {
            let mut plugin = state
                .history
                .plugins
                .get(id)
                .cloned()
                .unwrap_or_else(|| empty_plugin(*id));
            let previous = plugin.named_by;
            numbered.update_plugin(state, &mut plugin, &pending.action, prepared.loaded.get(id))?;
            if plugin.named_by == Some(numbered.write.registration.id) {
                numbered.write.registration.names.insert(
                    *id,
                    RegisteredName {
                        name: pending.name.clone(),
                        previous,
                    },
                );
            }
            if let Some(active) = plugin.active
                && active.registration == Some(numbered.write.registration.id)
            {
                numbered.write.registration.code.insert(
                    *id,
                    RegisteredCode {
                        generation: active.generation,
                        intent: pending.intent,
                    },
                );
            }
            numbered.write.plugins.push(plugin);
        }
        if let Some(config) = &prepared.plan.configuration {
            numbered.write.configuration = Some(ConfigGeneration {
                id: numbered.next,
                registration: numbered.write.registration.id,
                document: config.document.clone(),
                source: config.source,
                previous: state.history.active_config,
                created_at: numbered.write.registration.created_at,
            });
        }
        Ok(numbered)
    }

    fn add_generation(
        &mut self,
        plugin: PluginId,
        content: &Content,
        loaded: &Loaded,
    ) -> GenerationId {
        let id = self.next;
        self.next.0 += 1;
        self.write.generations.push(GenerationRecord {
            id,
            plugin,
            artifact: ContentHash::of(&content.bytes),
            settings: content.settings.clone(),
            origin: content.origin,
            status: GenerationStatus::Trial,
            failure: None,
            created_at: self.write.registration.created_at,
        });
        self.loaded.insert(id, loaded.clone());
        id
    }

    fn update_plugin(
        &mut self,
        state: &State,
        plugin: &mut PluginRecord,
        action: &Action,
        loaded: Option<&Loaded>,
    ) -> Result<(), RegistryError> {
        let registration = self.write.registration.id;
        match action {
            Action::Code(content) => {
                let loaded = loaded.ok_or_else(|| {
                    RegistryError::Intent(format!(
                        "code candidate for {} was not loaded",
                        plugin.id
                    ))
                })?;
                let id = self.add_generation(plugin.id, content, loaded);
                plugin.rollback_target = next_rollback(&state.history, plugin);
                if plugin.active.is_none() {
                    plugin.named_by = Some(registration);
                }
                plugin.active = Some(PluginActivation {
                    generation: id,
                    registration: Some(registration),
                });
            }
            Action::Existing(generation, loaded) => {
                plugin.rollback_target = next_rollback(&state.history, plugin);
                if plugin.active.is_none() {
                    plugin.named_by = Some(registration);
                }
                plugin.active = Some(PluginActivation {
                    generation: *generation,
                    registration: Some(registration),
                });
                self.loaded.insert(*generation, loaded.as_ref().clone());
            }
            Action::Remove => {
                plugin.rollback_target = next_rollback(&state.history, plugin);
                plugin.active = None;
                plugin.named_by = Some(registration);
            }
            Action::Rename => plugin.named_by = Some(registration),
        }
        Ok(())
    }
}

/// Evaluate the complete result, including removals and renames, without changing live routes.
fn contents(
    state: &State,
    numbered: &Numbered,
) -> Result<BTreeMap<PluginName, Option<Loaded>>, RegistryError> {
    let mut identities = state.history.plugins.clone();
    identities.extend(
        numbered
            .write
            .plugins
            .iter()
            .cloned()
            .map(|plugin| (plugin.id, plugin)),
    );
    let mut contents = BTreeMap::new();
    for plugin in identities.values() {
        let Some(active) = plugin.active else {
            continue;
        };
        let name = if plugin.named_by == Some(numbered.write.registration.id) {
            numbered.write.registration.names.get(&plugin.id)
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
        let loaded = numbered
            .loaded
            .get(&active.generation)
            .or(state.loaded.get(&active.generation))
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
        factory::retain(deps, state, &write.plugins).await?;
        for plugin in prepared.plan.plugins.values() {
            if let Action::Code(content) = &plugin.action {
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
