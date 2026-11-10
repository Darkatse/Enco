use super::super::configuration::Declaration;
use super::*;
use std::borrow::Cow;

pub(in crate::registry) struct Plan {
    pub expected: Expected,
    pub initiator: Initiator,
    pub configuration: Option<ConfigurationSource>,
    pub declaration: Declaration,
    pub previous_uses: Vec<Use>,
    pub plugins: BTreeMap<PluginId, PlannedPlugin>,
}

pub(in crate::registry) struct Content {
    /// Borrowed when embedded in the host binary; owned when built or read from the store.
    pub bytes: Cow<'static, [u8]>,
    pub settings: PluginSettings,
    pub origin: Origin,
}

pub(in crate::registry) struct PlannedPlugin {
    pub name: Option<PluginName>,
    pub action: Action,
    pub intent: Option<CodeIntent>,
}

pub(in crate::registry) enum Action {
    /// Content the host does not have, which brings a generation.
    Code(Content),
    /// This host's factory code with empty settings, which brings none.
    Factory(CodeRef, Box<Loaded>),
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
    if request.base != state.intent.registrations.keys().next_back().copied() {
        return Err(RegistryError::Conflict("registration baseline".into()));
    }
    if request.configuration.is_none() && request.plugins.is_empty() {
        return Err(RegistryError::EmptyRequest);
    }
    let declaration = match &request.configuration {
        Some(config) => configuration::declare_valid(&config.document, deps)?,
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
    let existing = identities.get(&name).copied();
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
            bytes: Cow::Owned(match artifact {
                ArtifactSource::Bytes(bytes) => bytes,
                ArtifactSource::Stored(hash) => deps.store.artifact(&hash).await?,
            }),
            settings,
            origin: Origin::Code,
        }),
        PluginChange::Factory => {
            let factory = factory.ok_or_else(|| RegistryError::NotFactory(name.clone()))?;
            factory_itself(state, factory, &settings).unwrap_or_else(|| {
                Action::Code(Content {
                    bytes: Cow::Borrowed(factory.artifact),
                    settings,
                    origin: Origin::Code,
                })
            })
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
        let Some(based_on) = state.active(id).cloned() else {
            return Err(rejected(Cause::SettingsWithoutCode {
                plugin: name.clone(),
            }));
        };
        // A plugin following factory code runs it as itself once its settings are emptied.
        let follows_factory = state.provenance(&based_on)?.source == CodeSource::Factory;
        let itself = deps
            .factory
            .iter()
            .find(|factory| factory.id == id)
            .filter(|_| follows_factory)
            .and_then(|factory| factory_itself(state, factory, &selected));
        let (intent, action) = match itself {
            Some(action) => (Some(CodeIntent::Factory), action),
            None => (
                None,
                Action::Code(Content {
                    bytes: artifact(deps, &state.artifact(&based_on)?).await?,
                    settings: selected,
                    origin: Origin::Settings { based_on },
                }),
            ),
        };
        plan.plugins.insert(
            id,
            PlannedPlugin {
                name: Some(name.clone()),
                intent,
                action,
            },
        );
    }
    Ok(())
}

/// This host's factory code run as itself: with empty settings it is host content, which brings
/// no generation; otherwise it is not.
fn factory_itself(
    state: &State,
    factory: &FactoryPlugin,
    settings: &PluginSettings,
) -> Option<Action> {
    (*settings == PluginSettings::default()).then(|| {
        let code = state.factory[&factory.id].clone();
        let loaded = state.loaded[&code].clone();
        Action::Factory(code, Box::new(loaded))
    })
}

/// Embedded bytes when this host ships them, otherwise the stored file.
pub(in crate::registry) async fn artifact(
    deps: &RegistryDeps,
    hash: &ContentHash,
) -> Result<Cow<'static, [u8]>, RegistryError> {
    match deps
        .factory
        .iter()
        .find(|factory| ContentHash::of(factory.artifact) == *hash)
    {
        Some(factory) => Ok(Cow::Borrowed(factory.artifact)),
        None => Ok(Cow::Owned(deps.store.artifact(hash).await?)),
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
            Action::Factory(_, existing) => probe(existing.as_ref().clone(), name, candidate).await,
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
            Action::Factory(..) => Some(Proposed::Factory),
            Action::Code(content) => Some(match (self.intent, &content.origin) {
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
/// Every mapped name must have only fixed-origin placements, and every required credential
/// with only fixed-origin placements must be mapped. The rule joins code and settings, so it is checked here,
/// where both are accepted together; a build loads code alone, with no settings to check.
fn secrets_fit(declared: &[Credential], settings: &PluginSettings) -> Result<(), LoadError> {
    let fixed = |name: &String| {
        declared
            .iter()
            .any(|credential| credential.name == *name && !credential.caller_selected())
    };
    if let Some(name) = settings.secrets.keys().find(|name| !fixed(name)) {
        return Err(LoadError::Settings(format!(
            "secret {name} has no declared plugin-owned origin"
        )));
    }
    if let Some(credential) = declared.iter().find(|credential| {
        !credential.caller_selected()
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
