use super::super::configuration::Declaration;
use super::*;
use std::borrow::Cow;

/// A registration checked against one state; `L` is what loading new code produced, nothing
/// until it is prepared.
pub(in crate::registry) struct Plan<L = ()> {
    pub expected: Expected,
    pub initiator: Initiator,
    pub configuration: Option<ConfigurationSource>,
    pub declaration: Declaration,
    pub previous_uses: Vec<Use>,
    pub plugins: BTreeMap<PluginId, Action<L>>,
}

pub(in crate::registry) struct Content {
    /// Borrowed when embedded in the host binary; owned when built or read from the store.
    pub bytes: Cow<'static, [u8]>,
    pub settings: PluginSettings,
    pub origin: Origin,
}

/// What a registration does to one plugin.
pub(in crate::registry) enum Action<L> {
    /// Content the host does not have, under `name`, which brings a generation.
    Code {
        name: PluginName,
        content: Content,
        loaded: L,
    },
    /// This host's factory code run as itself under `name`, which brings none.
    Factory {
        name: PluginName,
        artifact: ContentHash,
        loaded: Box<Loaded>,
    },
    /// Removes the plugin and releases its name.
    Remove,
    /// Gives the plugin another name.
    Rename { to: PluginName },
}

impl<L> Action<L> {
    /// The plugin's name after the registration; absent once removed.
    pub fn name(&self) -> Option<&PluginName> {
        match self {
            Self::Code { name, .. } | Self::Factory { name, .. } | Self::Rename { to: name } => {
                Some(name)
            }
            Self::Remove => None,
        }
    }
}

/// A plan whose new code is loaded and probed.
pub(in crate::registry) struct Prepared {
    pub plan: Plan<Box<Loaded>>,
    pub probes: Vec<Probe>,
}

impl Prepared {
    /// The code this registration gives the plugin named `name`.
    pub fn loaded_for(&self, name: &PluginName) -> Option<&Loaded> {
        self.plan.plugins.values().find_map(|action| match action {
            Action::Code {
                name: to, loaded, ..
            }
            | Action::Factory {
                name: to, loaded, ..
            } if to == name => Some(loaded.as_ref()),
            _ => None,
        })
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
    let settings = plan
        .declaration
        .plugins
        .get(&name)
        .cloned()
        .unwrap_or_default();
    let action = match change {
        PluginChange::Code { source, artifact } => Action::Code {
            name,
            content: Content {
                bytes: Cow::Owned(match artifact {
                    ArtifactSource::Bytes(bytes) => bytes,
                    ArtifactSource::Stored(hash) => deps.store.artifact(&hash).await?,
                }),
                settings,
                origin: Origin::Code {
                    intent: CodeIntent::Directory { source },
                },
            },
            loaded: (),
        },
        PluginChange::Factory => {
            let factory = factory.ok_or_else(|| RegistryError::NotFactory(name.clone()))?;
            match factory_itself(state, factory, &settings) {
                Some(action) => action,
                None => Action::Code {
                    name,
                    content: Content {
                        bytes: Cow::Borrowed(factory.artifact),
                        settings,
                        origin: Origin::Code {
                            intent: CodeIntent::Factory,
                        },
                    },
                    loaded: (),
                },
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
            Action::Rename { to }
        }
    };
    plan.plugins.insert(id, action);
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
                    .find_map(|(id, action)| (action.name() == Some(name)).then_some(*id))
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
        let Some(based_on) = state.active(id) else {
            return Err(rejected(Cause::SettingsWithoutCode {
                plugin: name.clone(),
            }));
        };
        // A plugin following factory code runs it as itself once its settings are emptied.
        let follows_factory = state.provenance(based_on)?.source == CodeSource::Factory;
        let itself = deps
            .factory
            .iter()
            .find(|factory| factory.id == id)
            .filter(|_| follows_factory)
            .and_then(|factory| factory_itself(state, factory, &selected));
        let action = match itself {
            Some(action) => action,
            None => Action::Code {
                name: name.clone(),
                content: Content {
                    bytes: artifact(deps, &state.artifact(based_on)?).await?,
                    settings: selected,
                    origin: Origin::Settings {
                        based_on: based_on.code,
                    },
                },
                loaded: (),
            },
        };
        plan.plugins.insert(id, action);
    }
    Ok(())
}

/// This host's factory code run as itself: with empty settings it is host content, which brings
/// no generation; otherwise it is not.
fn factory_itself(
    state: &State,
    factory: &FactoryPlugin,
    settings: &PluginSettings,
) -> Option<Action<()>> {
    (*settings == PluginSettings::default()).then(|| {
        let artifact = state.factory[&factory.id];
        let code = CodeRef {
            plugin: factory.id,
            code: PluginCode::Factory { artifact },
        };
        Action::Factory {
            name: factory.name.clone(),
            artifact,
            loaded: Box::new(state.loaded[&code].clone()),
        }
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

/// Load and probe every candidate outside the registry lock; any failure rejects the whole
/// registration with every cause found.
pub(in crate::registry) async fn prepare(
    deps: &RegistryDeps,
    plan: Plan,
) -> Result<Prepared, RegistryError> {
    let Plan {
        expected,
        initiator,
        configuration,
        declaration,
        previous_uses,
        plugins,
    } = plan;
    let mut prepared = BTreeMap::new();
    let mut causes = Vec::new();
    for (id, action) in plugins {
        let result = match action {
            Action::Code {
                name,
                content,
                loaded: (),
            } => load(deps.runtime.as_ref(), &content, &name)
                .await
                .map(|loaded| Action::Code {
                    name,
                    content,
                    loaded: Box::new(loaded),
                }),
            Action::Factory {
                name,
                artifact,
                loaded,
            } => probe(*loaded, &name, Proposed::Factory)
                .await
                .map(|loaded| Action::Factory {
                    name,
                    artifact,
                    loaded: Box::new(loaded),
                }),
            Action::Remove => Ok(Action::Remove),
            Action::Rename { to } => Ok(Action::Rename { to }),
        };
        match result {
            Ok(action) => {
                prepared.insert(id, action);
            }
            Err(cause) => causes.push(cause),
        }
    }
    if !causes.is_empty() {
        return Err(RegistryError::Rejected(causes));
    }
    Ok(Prepared {
        plan: Plan {
            expected,
            initiator,
            configuration,
            declaration,
            previous_uses,
            plugins: prepared,
        },
        probes: Vec::new(),
    })
}

/// What new content changes, which decides the part a load failure concerns.
fn proposed(origin: &Origin) -> Proposed {
    match origin {
        Origin::Code {
            intent: CodeIntent::Directory { .. },
        } => Proposed::Code,
        Origin::Code {
            intent: CodeIntent::Factory,
        } => Proposed::Factory,
        Origin::Settings { .. } => Proposed::Settings,
    }
}

async fn load(
    runtime: &dyn Runtime,
    content: &Content,
    name: &PluginName,
) -> Result<Loaded, Cause> {
    let candidate = proposed(&content.origin);
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
