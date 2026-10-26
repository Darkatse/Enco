use super::*;
use crate::{Acceptance, ConnectionWrite, NewSession};

/// The registry owns the named settings sections of plugins.
pub const PLUGIN_SETTINGS_TABLE: ConfigTable = ConfigTable::Named("plugin");

/// Parse plugin settings using the same ownership boundary as registration.
pub fn plugin_settings(
    document: &serde_json::Value,
) -> Result<BTreeMap<PluginName, PluginSettings>, SettingsError> {
    document.get(PLUGIN_SETTINGS_TABLE.name()).map_or_else(
        || Ok(BTreeMap::new()),
        |value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                SettingsError::new(PLUGIN_SETTINGS_TABLE.name(), error.to_string())
            })
        },
    )
}

pub(super) struct Declaration {
    pub kernel: KernelSettings,
    pub uses: Vec<Use>,
    pub invalid: Vec<SettingsError>,
    pub plugins: BTreeMap<PluginName, PluginSettings>,
}

/// Ownership is a property of the composition, independent of any document.
pub(super) fn check_owners(owners: &[TableOwner]) -> Result<(), RegistryError> {
    let mut names = std::collections::BTreeSet::new();
    for table in tables(owners) {
        if !names.insert(table.name()) {
            return Err(RegistryError::TableOwner(table.name().into()));
        }
    }
    Ok(())
}

/// Evaluate facts; each caller decides whether invalid host tables prevent its operation.
pub(super) fn declare(
    document: &serde_json::Value,
    deps: &RegistryDeps,
) -> Result<Declaration, SettingsError> {
    let owners = &deps.owners;
    let kernel = KernelSettings::from_document(document, &deps.composer)?;
    for (name, profile) in &kernel.profiles {
        if profile.requires_lifeline
            && let Some(tools) = &profile.tools
        {
            let missing: Vec<_> = deps
                .lifeline
                .iter()
                .filter(|name| !tools.contains(*name))
                .collect();
            if !missing.is_empty() {
                return Err(SettingsError::new(
                    format!("profile.{name}"),
                    format!(
                        "tools must include the required lifeline: {}",
                        missing
                            .iter()
                            .map(|name| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    let tables = document
        .as_object()
        .ok_or_else(|| SettingsError::new("config", "expected a document of named tables"))?;
    let mut invalid = tables
        .keys()
        .filter(|table| !self::tables(owners).any(|known| known.name() == table.as_str()))
        .map(|table| SettingsError::new(table, "no table owner"))
        .collect::<Vec<_>>();
    let mut uses = kernel.uses();
    for owner in owners {
        match (owner.declare)(document, &kernel) {
            Ok(declared) => uses.extend(declared),
            Err(error) => invalid.push(error),
        }
    }
    Ok(Declaration {
        kernel,
        uses,
        invalid,
        plugins: plugin_settings(document)?,
    })
}

fn tables(owners: &[TableOwner]) -> impl Iterator<Item = ConfigTable> + '_ {
    KernelSettings::TABLES
        .into_iter()
        .chain([PLUGIN_SETTINGS_TABLE])
        .chain(owners.iter().flat_map(|owner| owner.tables.iter().copied()))
}

/// Changes use the same section ownership as declaration and validation.
pub(super) fn changed_sections(
    before: Option<&serde_json::Value>,
    after: &serde_json::Value,
    owners: &[TableOwner],
) -> Vec<String> {
    let before_table = |name: &str| before.and_then(|document| document.get(name));
    let keys: std::collections::BTreeSet<_> = before
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|document| document.keys())
        .chain(
            after
                .as_object()
                .into_iter()
                .flat_map(|document| document.keys()),
        )
        .collect();
    let mut changed = Vec::new();
    for name in keys {
        let old = before_table(name);
        let new = after.get(name);
        if old == new {
            continue;
        }
        let named = tables(owners)
            .find(|table| table.name() == name)
            .is_some_and(|table| matches!(table, ConfigTable::Named(_)));
        if named
            && old.is_none_or(serde_json::Value::is_object)
            && new.is_none_or(serde_json::Value::is_object)
        {
            let entries: std::collections::BTreeSet<_> = old
                .and_then(serde_json::Value::as_object)
                .into_iter()
                .flat_map(|object| object.keys())
                .chain(
                    new.and_then(serde_json::Value::as_object)
                        .into_iter()
                        .flat_map(|object| object.keys()),
                )
                .collect();
            changed.extend(
                entries
                    .into_iter()
                    .filter(|entry| {
                        old.and_then(|value| value.get(*entry))
                            != new.and_then(|value| value.get(*entry))
                    })
                    .map(|entry| format!("{name}.{entry}")),
            );
        } else {
            // Historical tables no longer understood by this host retain their recorded top-level name.
            changed.push(name.clone());
        }
    }
    changed
}

/// Intentional registration accepts only content the current host fully interprets.
pub(super) fn declare_valid(
    document: &serde_json::Value,
    deps: &RegistryDeps,
) -> Result<Declaration, RegistryError> {
    let declaration = declare(document, deps).map_err(|error| rejected(Cause::Table(error)))?;
    if declaration.invalid.is_empty() {
        Ok(declaration)
    } else {
        Err(RegistryError::Rejected(
            declaration
                .invalid
                .iter()
                .cloned()
                .map(Cause::Table)
                .collect(),
        ))
    }
}

impl Declaration {
    pub fn from_active(config: &ActiveConfig) -> Self {
        Self {
            kernel: config.kernel.clone(),
            uses: config.uses.clone(),
            invalid: config.invalid.clone(),
            plugins: config.plugins.clone(),
        }
    }

    /// The only way to activate content: with the declarations evaluated from it.
    pub fn into_active(self, record: &ConfigGeneration) -> ActiveConfig {
        ActiveConfig {
            generation: record.id,
            document: record.document.clone(),
            kernel: self.kernel,
            uses: self.uses,
            invalid: self.invalid,
            plugins: self.plugins,
        }
    }
}

/// Sessions name profiles, so a configuration cannot remove a profile still in use.
pub(super) async fn check_profiles(
    deps: &RegistryDeps,
    kernel: &KernelSettings,
) -> Result<(), RegistryError> {
    let mut stranded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for session in deps.store.sessions().await? {
        if !kernel.profiles.contains_key(&session.profile) {
            stranded
                .entry(session.profile)
                .or_default()
                .push(session.name);
        }
    }
    if stranded.is_empty() {
        Ok(())
    } else {
        Err(RegistryError::Rejected(
            stranded
                .into_iter()
                .map(|(profile, sessions)| Cause::ProfileInUse { profile, sessions })
                .collect(),
        ))
    }
}

impl Registry {
    /// The host's repair set, also used when validating profile tool restrictions.
    pub fn lifeline(&self) -> &[String] {
        &self.deps.lifeline
    }

    /// Session creation and profile selection observe one registered configuration.
    pub(crate) async fn session_view(
        &self,
        session: SessionId,
    ) -> Result<(SessionRecord, Arc<View>), RegistryError> {
        let _state = self.state.lock().await;
        let session = self
            .deps
            .store
            .session(session)
            .await?
            .ok_or(StoreError::UnknownSession(session))?;
        Ok((session, self.view()))
    }

    /// Accept input that creates a Session, under the committer that also changes profiles.
    ///
    /// The Session thus never starts with a profile that a concurrent registration removes.
    pub(crate) async fn accept_creating(
        &self,
        session: &NewSession,
        events: &[Event],
        connection: Option<&ConnectionWrite>,
    ) -> Result<Acceptance, RegistryError> {
        let _state = self.state.lock().await;
        if !self
            .view()
            .config
            .kernel
            .profiles
            .contains_key(&session.profile)
        {
            return Err(RegistryError::UnknownProfile(session.profile.clone()));
        }
        Ok(self.deps.store.accept(events, connection).await?)
    }

    /// Profile assignments share the registry's committer with configuration changes.
    pub async fn set_profile(
        &self,
        session: SessionId,
        profile: &str,
    ) -> Result<(), RegistryError> {
        let _state = self.state.lock().await;
        if !self.view().config.kernel.profiles.contains_key(profile) {
            return Err(RegistryError::UnknownProfile(profile.into()));
        }
        self.deps.store.set_profile(session, profile).await?;
        Ok(())
    }
}
