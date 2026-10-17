use super::*;

pub(super) struct Declaration {
    pub kernel: KernelSettings,
    pub uses: Vec<Use>,
    pub invalid: Vec<SettingsError>,
    pub plugins: BTreeMap<String, PluginSettings>,
}

/// Ownership is a property of the composition, independent of any document.
pub(super) fn check_owners(owners: &[TableOwner]) -> Result<(), RegistryError> {
    let mut tables = std::collections::BTreeSet::from(KernelSettings::TABLES);
    tables.insert("plugin");
    for table in owners.iter().flat_map(|owner| owner.tables) {
        if !tables.insert(*table) {
            return Err(RegistryError::TableOwner((*table).into()));
        }
    }
    Ok(())
}

/// Evaluate facts; each caller decides whether invalid host tables prevent its operation.
pub(super) fn declare(
    document: &serde_json::Value,
    owners: &[TableOwner],
) -> Result<Declaration, SettingsError> {
    let kernel = KernelSettings::from_document(document)?;
    let tables = document
        .as_object()
        .ok_or_else(|| SettingsError::new("config", "expected a document of named tables"))?;
    let mut invalid = tables
        .keys()
        .filter(|table| {
            table.as_str() != "plugin"
                && !KernelSettings::TABLES.contains(&table.as_str())
                && !owners
                    .iter()
                    .any(|owner| owner.tables.contains(&table.as_str()))
        })
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
        plugins: document.get("plugin").map_or_else(
            || Ok(BTreeMap::new()),
            |value| {
                serde_json::from_value(value.clone())
                    .map_err(|error| SettingsError::new("plugin", error.to_string()))
            },
        )?,
    })
}

/// Intentional registration accepts only content the current host fully interprets.
pub(super) fn declare_valid(
    document: &serde_json::Value,
    owners: &[TableOwner],
) -> Result<Declaration, RegistryError> {
    let declaration = declare(document, owners).map_err(|error| rejected("config", error))?;
    match declaration.invalid.first() {
        Some(error) => Err(rejected("config", error)),
        None => Ok(declaration),
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
    let invalid: Vec<_> = deps
        .store
        .sessions()
        .await?
        .into_iter()
        .filter(|session| !kernel.profiles.contains_key(&session.profile))
        .map(|session| {
            format!(
                "{} ({}, profile {})",
                session.name, session.id, session.profile
            )
        })
        .collect();
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(rejected(
            "config",
            format!(
                "profiles are still used by Sessions: {}",
                invalid.join(", ")
            ),
        ))
    }
}

impl Registry {
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
