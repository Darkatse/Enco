use super::super::configuration::Declaration;
use super::*;

pub(in crate::registry) struct Plan {
    pub expected: Expected,
    pub initiator: Initiator,
    pub configuration: Option<ConfigurationSource>,
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
    pub name: Option<PluginName>,
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
    pub fn loaded_for(&self, name: &PluginName) -> Option<&Loaded> {
        let (id, _) = self
            .plan
            .plugins
            .iter()
            .find(|(_, plugin)| plugin.name.as_ref() == Some(name))?;
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
        return Err(RegistryError::EmptyRequest);
    }
    let declaration = match &request.configuration {
        Some(config) => configuration::declare_valid(&config.document, &deps.owners)?,
        None => current
            .map(Declaration::from_active)
            .ok_or_else(|| RegistryError::Bootstrap("configuration document is missing".into()))?,
    };
    let mut plan = Plan {
        expected: Expected::capture(state),
        initiator: request.initiator,
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
    identities: &BTreeMap<PluginName, PluginId>,
    current: Option<&ActiveConfig>,
    plan: &mut Plan,
    name: PluginName,
    change: PluginChange,
) -> Result<(), RegistryError> {
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
        PluginChange::Code { source, .. } => Some(CodeIntent::Directory { source: *source }),
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
        PluginChange::Code { artifact, .. } => Action::Code(Content {
            bytes: match artifact {
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
            let factory = factory.ok_or_else(|| RegistryError::NotFactory(name.clone()))?;
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
            return Err(RegistryError::FactoryRemoval(name));
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
    identities: &BTreeMap<PluginName, PluginId>,
    current: Option<&ActiveConfig>,
    name: &PluginName,
    to: &PluginName,
) -> Result<(), RegistryError> {
    let id = *identities
        .get(name)
        .ok_or_else(|| RegistryError::UnknownPlugin(name.clone()))?;
    for reserved in [name, to] {
        if deps.factory.iter().any(|factory| factory.name == *reserved) {
            return Err(rejected(Cause::NameReserved {
                name: reserved.clone(),
            }));
        }
    }
    if let Some(current) = current {
        let mut by: Vec<_> = users(&current.uses, name)
            .into_iter()
            .map(|user| Referrer::User { user })
            .collect();
        if current.plugins.contains_key(name) {
            by.push(Referrer::Settings);
        }
        if !by.is_empty() {
            return Err(rejected(Cause::Referenced {
                name: name.clone(),
                by,
            }));
        }
    }
    if identities.get(to).is_some_and(|other| *other != id)
        || state
            .running_names()
            .get(to)
            .is_some_and(|other| *other != id)
    {
        return Err(rejected(Cause::NameTaken { name: to.clone() }));
    }
    Ok(())
}

async fn plan_settings(
    deps: &RegistryDeps,
    state: &State,
    identities: &BTreeMap<PluginName, PluginId>,
    plan: &mut Plan,
) -> Result<(), RegistryError> {
    if plan.configuration.is_none() {
        return Ok(());
    }
    let previous = state
        .registered_config()
        .map(|config| plugin_settings(&config.document))
        .transpose()
        .map_err(RegistryError::Config)?
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
            .ok_or_else(|| {
                rejected(Cause::SettingsWithoutPlugin {
                    plugin: name.clone(),
                })
            })?;
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
                return Err(rejected(Cause::SettingsWithoutCode {
                    plugin: name.clone(),
                }));
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
    let mut causes = Vec::new();
    for (id, plugin) in &plan.plugins {
        let (Some(name), Some(candidate)) = (&plugin.name, plugin.candidate()) else {
            continue;
        };
        let result = match &plugin.action {
            Action::Code(content) => load(deps.runtime.as_ref(), content, name, candidate).await,
            Action::Existing(_, existing) => probe(existing.clone(), name, candidate).await,
            Action::Remove | Action::Rename => continue,
        };
        match result {
            Ok(value) => {
                loaded.insert(*id, value);
            }
            Err(cause) => causes.push(cause),
        }
    }
    if !causes.is_empty() {
        return Err(RegistryError::Rejected(causes));
    }
    Ok(Prepared {
        plan,
        loaded,
        probes: Vec::new(),
    })
}

impl PlannedPlugin {
    /// What the candidate changes, which decides the part a load failure concerns.
    fn candidate(&self) -> Option<Proposed> {
        match &self.action {
            Action::Existing(..) => Some(Proposed::Factory),
            Action::Code(content) => Some(match (self.intent, content.origin) {
                (Some(CodeIntent::Factory), _) => Proposed::Factory,
                (_, Origin::Settings { .. }) => Proposed::Settings,
                (Some(CodeIntent::Directory { .. }) | None, _) => Proposed::Code,
            }),
            Action::Remove | Action::Rename => None,
        }
    }
}

async fn load(
    runtime: &dyn Runtime,
    content: &Content,
    name: &PluginName,
    candidate: Proposed,
) -> Result<Loaded, Cause> {
    let rejected = |error| Cause::Load {
        plugin: name.clone(),
        candidate,
        error,
    };
    let loaded = runtime
        .load(&content.bytes, &content.settings)
        .await
        .map_err(rejected)?;
    secrets_fit(&loaded.credentials, &content.settings).map_err(rejected)?;
    probe(loaded, name, candidate).await
}

/// Whether the owner's secret mapping fits the credentials the code declares.
///
/// Every mapped name must be a credential bound to a fixed origin, and every required credential
/// with a fixed origin must be mapped. The rule joins code and settings, so it is checked here,
/// where both are accepted together; a build loads code alone, with no settings to check.
fn secrets_fit(declared: &[Credential], settings: &PluginSettings) -> Result<(), LoadError> {
    let fixed = |name: &String| {
        declared
            .iter()
            .any(|credential| credential.name == *name && credential.origin.is_some())
    };
    if let Some(name) = settings.secrets.keys().find(|name| !fixed(name)) {
        return Err(LoadError::Settings(format!(
            "secret {name} has no declared plugin-owned origin"
        )));
    }
    if let Some(credential) = declared.iter().find(|credential| {
        credential.origin.is_some()
            && credential.required
            && !settings.secrets.contains_key(&credential.name)
    }) {
        return Err(LoadError::Settings(format!(
            "required credential {} has no secret mapping",
            credential.name
        )));
    }
    Ok(())
}

async fn probe(loaded: Loaded, name: &PluginName, candidate: Proposed) -> Result<Loaded, Cause> {
    loaded
        .lifecycle
        .probe()
        .await
        .map_err(|failure| Cause::Probe {
            plugin: name.clone(),
            candidate,
            failure,
        })?;
    Ok(loaded)
}
