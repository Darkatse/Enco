use super::super::configuration::Declaration;
use super::*;

pub(in crate::registry) struct Plan {
    pub expected: Expected,
    pub revision: String,
    pub configuration: Option<serde_json::Value>,
    pub declaration: Declaration,
    pub previous_uses: Vec<Use>,
    pub plugins: BTreeMap<PluginId, PlannedPlugin>,
}

pub(in crate::registry) struct Content {
    pub bytes: Vec<u8>,
    pub settings: PluginSettings,
    pub origin: Origin,
}

pub(in crate::registry) struct PlannedPlugin {
    pub name: Option<String>,
    pub action: Action,
    pub intent: Option<CodeIntent>,
}

pub(in crate::registry) enum Action {
    Code(Content),
    Existing(GenerationId, Loaded),
    Remove,
    Rename,
}

pub(in crate::registry) struct Prepared {
    pub plan: Plan,
    pub loaded: BTreeMap<PluginId, Loaded>,
    pub probes: Vec<Probe>,
}

impl Prepared {
    pub fn loaded_for(&self, name: &str) -> Option<&Loaded> {
        let (id, _) = self
            .plan
            .plugins
            .iter()
            .find(|(_, plugin)| plugin.name.as_deref() == Some(name))?;
        self.loaded.get(id)
    }
}

pub(in crate::registry) async fn plan(
    deps: &RegistryDeps,
    state: &State,
    current: Option<&ActiveConfig>,
    request: RegistrationRequest,
) -> Result<Plan, RegistryError> {
    if request.base != state.history.registrations.keys().next_back().copied() {
        return Err(RegistryError::Conflict("registration baseline".into()));
    }
    if request.configuration.is_none() && request.plugins.is_empty() {
        return Err(rejected("registration", "no changes supplied"));
    }
    let declaration = match &request.configuration {
        Some(document) => configuration::declare_valid(document, &deps.owners)?,
        None => current
            .map(Declaration::from_active)
            .ok_or_else(|| RegistryError::Bootstrap("configuration document is missing".into()))?,
    };
    let mut plan = Plan {
        expected: Expected::capture(state),
        revision: request.revision,
        configuration: request.configuration,
        declaration,
        previous_uses: current.map_or_else(Vec::new, |config| config.uses.clone()),
        plugins: BTreeMap::new(),
    };
    let identities = state.registered_identities(&deps.factory);
    for (name, change) in request.plugins {
        plan_plugin(deps, state, &identities, current, &mut plan, name, change).await?;
    }
    plan_settings(deps, state, &identities, &mut plan).await?;
    Ok(plan)
}

async fn plan_plugin(
    deps: &RegistryDeps,
    state: &State,
    identities: &BTreeMap<String, PluginId>,
    current: Option<&ActiveConfig>,
    plan: &mut Plan,
    name: String,
    change: PluginChange,
) -> Result<(), RegistryError> {
    validate_name(&name)?;
    let factory = deps.factory.iter().find(|factory| factory.name == name);
    let registered = identities.get(&name).copied();
    // A direct artifact replaces the code running under the name; intent follows registered names.
    let existing = if matches!(change, PluginChange::Artifact(_)) {
        state.running_names().get(&name).copied().or(registered)
    } else {
        registered
    };
    let id = existing.unwrap_or_default();
    let intent = match &change {
        PluginChange::Code(_) => Some(CodeIntent::Directory),
        PluginChange::Factory => Some(CodeIntent::Factory),
        _ => None,
    };
    let settings = plan
        .declaration
        .plugins
        .get(&name)
        .cloned()
        .unwrap_or_default();
    let action = match change {
        PluginChange::Code(source) => Action::Code(Content {
            bytes: match source {
                ArtifactSource::Bytes(bytes) => bytes,
                ArtifactSource::Stored(hash) => deps.store.artifact(&hash).await?,
            },
            settings,
            origin: Origin::Code {
                registration: plan.expected.next,
            },
        }),
        PluginChange::Artifact(bytes) => Action::Code(Content {
            bytes,
            settings,
            origin: Origin::Artifact {
                registration: plan.expected.next,
            },
        }),
        PluginChange::Factory => {
            let factory = factory.ok_or_else(|| rejected(&name, "not a factory plugin"))?;
            let generation = state.factory[&factory.id];
            if settings == PluginSettings::default() {
                let loaded = state.loaded[&generation].clone();
                Action::Existing(generation, loaded)
            } else {
                Action::Code(Content {
                    bytes: factory.artifact.clone(),
                    settings,
                    origin: Origin::Code {
                        registration: plan.expected.next,
                    },
                })
            }
        }
        PluginChange::Remove if factory.is_some() => {
            return Err(rejected(
                &name,
                "factory plugins cannot be removed; register factory intent",
            ));
        }
        PluginChange::Remove => {
            if existing.is_none() {
                return Err(RegistryError::UnknownPlugin(name));
            }
            Action::Remove
        }
        PluginChange::Rename(to) => {
            check_rename(deps, state, identities, current, &name, &to)?;
            plan.plugins.insert(
                id,
                PlannedPlugin {
                    name: Some(to),
                    action: Action::Rename,
                    intent: None,
                },
            );
            return Ok(());
        }
    };
    let name = (!matches!(action, Action::Remove)).then_some(name);
    plan.plugins.insert(
        id,
        PlannedPlugin {
            name,
            action,
            intent,
        },
    );
    Ok(())
}

pub(in crate::registry) fn check_rename(
    deps: &RegistryDeps,
    state: &State,
    identities: &BTreeMap<String, PluginId>,
    current: Option<&ActiveConfig>,
    name: &str,
    to: &str,
) -> Result<(), RegistryError> {
    validate_name(name)?;
    validate_name(to)?;
    let id = *identities
        .get(name)
        .ok_or_else(|| RegistryError::UnknownPlugin(name.into()))?;
    if deps
        .factory
        .iter()
        .any(|factory| factory.name == name || factory.name == to)
    {
        return Err(rejected(name, "factory names are reserved"));
    }
    if let Some(current) = current {
        let mut references = users(&current.uses, name);
        if current.plugins.contains_key(name) {
            references.push(format!("plugin.{name}"));
        }
        if !references.is_empty() {
            return Err(rejected(
                name,
                format!("still referenced by {}", references.join(", ")),
            ));
        }
    }
    if identities.get(to).is_some_and(|other| *other != id)
        || state
            .running_names()
            .get(to)
            .is_some_and(|other| *other != id)
    {
        return Err(rejected(to, "name is already registered"));
    }
    Ok(())
}

async fn plan_settings(
    deps: &RegistryDeps,
    state: &State,
    identities: &BTreeMap<String, PluginId>,
    plan: &mut Plan,
) -> Result<(), RegistryError> {
    if plan.configuration.is_none() {
        return Ok(());
    }
    let previous = state
        .registered_config()
        .map(|config| settings(&config.document))
        .transpose()?
        .unwrap_or_default();
    let names: std::collections::BTreeSet<_> = previous
        .keys()
        .chain(plan.declaration.plugins.keys())
        .collect();
    for name in names {
        if previous.get(name) == plan.declaration.plugins.get(name) {
            continue;
        }
        let id = identities
            .get(name)
            .copied()
            .or_else(|| {
                plan.plugins
                    .iter()
                    .find_map(|(id, plugin)| (plugin.name.as_ref() == Some(name)).then_some(*id))
            })
            .ok_or_else(|| rejected(name, "settings require a plugin in this registration"))?;
        if plan.plugins.contains_key(&id) {
            continue;
        }
        let selected = plan
            .declaration
            .plugins
            .get(name)
            .cloned()
            .unwrap_or_default();
        let (bytes, based_on) = match state.active(id) {
            Some(record) => (artifact(deps, record).await?, record.id),
            None => {
                return Err(rejected(
                    name,
                    "no code is running; register source together with these settings",
                ));
            }
        };
        plan.plugins.insert(
            id,
            PlannedPlugin {
                name: Some(name.clone()),
                intent: None,
                action: Action::Code(Content {
                    bytes,
                    settings: selected,
                    origin: Origin::Settings {
                        registration: plan.expected.next,
                        based_on,
                    },
                }),
            },
        );
    }
    Ok(())
}

pub(in crate::registry) async fn artifact(
    deps: &RegistryDeps,
    record: &GenerationRecord,
) -> Result<Vec<u8>, RegistryError> {
    match deps
        .factory
        .iter()
        .find(|factory| ContentHash::of(&factory.artifact) == record.artifact)
    {
        Some(factory) => Ok(factory.artifact.clone()),
        None => Ok(deps.store.artifact(&record.artifact).await?),
    }
}

pub(in crate::registry) async fn prepare(
    deps: &RegistryDeps,
    plan: Plan,
) -> Result<Prepared, RegistryError> {
    let mut loaded = BTreeMap::new();
    let mut errors = Vec::new();
    for (id, plugin) in &plan.plugins {
        let candidate = match &plugin.action {
            Action::Code(content) => Some(load(deps.runtime.as_ref(), content).await),
            Action::Existing(_, loaded) => Some(probe(loaded.clone()).await),
            Action::Remove | Action::Rename => None,
        };
        if let Some(candidate) = candidate {
            match candidate {
                Ok(value) => {
                    loaded.insert(*id, value);
                }
                Err(error) => errors.push(format!(
                    "{}: {error}",
                    plugin.name.as_deref().unwrap_or("removed plugin")
                )),
            }
        }
    }
    if !errors.is_empty() {
        return Err(rejected("registration", errors.join("\n")));
    }
    Ok(Prepared {
        plan,
        loaded,
        probes: Vec::new(),
    })
}

async fn load(runtime: &dyn Runtime, content: &Content) -> Result<Loaded, String> {
    let loaded = runtime
        .load(&content.bytes, &content.settings)
        .await
        .map_err(|error| error.to_string())?;
    probe(loaded).await
}

async fn probe(loaded: Loaded) -> Result<Loaded, String> {
    loaded
        .lifecycle
        .probe()
        .await
        .map_err(|failure| format!("probe failed: {}: {}", failure.code, failure.message))?;
    Ok(loaded)
}
