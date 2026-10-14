use super::probe;
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
    /// Validate changed content and new imports against the final composition under the committer.
    fn admit(
        &self,
        state: &State,
        host_tool_names: &[String],
        prepared: &BTreeMap<String, registration::Prepared>,
    ) -> Result<admission::Admission, RegistryError> {
        let candidates = prepared
            .iter()
            .map(|(name, prepared)| (name.clone(), prepared.loaded.clone()))
            .collect();
        let before = admission::evaluate(&state.plugins, host_tool_names, &BTreeMap::new());
        let admission = admission::evaluate(&state.plugins, host_tool_names, &candidates);
        let changed: Vec<_> = prepared.keys().cloned().collect();
        admission.accept_change(&before, &changed, &self.uses, &state.config.uses)?;
        Ok(admission)
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

/// Number and persist new content, keeping the in-memory history in step with the Store.
pub(super) async fn register(
    store: &dyn Store,
    configs: &mut BTreeMap<GenerationId, ConfigGeneration>,
    plugins: &mut state::Plugins,
    prepared: BTreeMap<String, registration::Prepared>,
    document: serde_json::Value,
    created_at: DateTime<Utc>,
) -> Result<ConfigGeneration, RegistryError> {
    let pending: Vec<_> = prepared
        .values()
        .map(|candidate| candidate.generation.clone())
        .collect();
    let (id, ids) = store
        .insert_config_generation(&document, created_at, &pending)
        .await?;
    let record = ConfigGeneration {
        id,
        document,
        created_at,
    };
    for (candidate, id) in prepared.into_values().zip(ids) {
        plugins.registered(candidate.generation.numbered(id), candidate.loaded);
    }
    configs.insert(id, record.clone());
    Ok(record)
}

impl Registry {
    /// Capture the Session's profile reference and its configuration under the same committer.
    pub(crate) async fn session_view(
        &self,
        session: SessionId,
    ) -> Result<(SessionRecord, Arc<View>), RegistryError> {
        let _state = self.state.lock().await;
        let session = self
            .store
            .session(session)
            .await?
            .ok_or(StoreError::UnknownSession(session))?;
        Ok((session, self.view()))
    }

    /// Compare intent to history without using intent to change runtime behavior.
    pub async fn config_divergence(
        &self,
        document: Result<serde_json::Value, String>,
    ) -> ConfigDivergence {
        let document = match document {
            Ok(document) => document,
            Err(reason) => {
                return ConfigDivergence::Unapplied {
                    reason: Some(reason),
                };
            }
        };
        let state = self.state.lock().await;
        if document == state.config.document {
            let active = state.settings_divergence();
            return if active.is_empty() {
                ConfigDivergence::None
            } else {
                ConfigDivergence::PluginSettings { active }
            };
        }
        let Some(matching) = state
            .configs
            .values()
            .rev()
            .find(|record| record.document == document)
        else {
            return ConfigDivergence::Unapplied { reason: None };
        };
        let active = state.config.generation;
        let reason = if active < matching.id {
            format!("mechanical rollback activated earlier configuration {active}")
        } else if let Some(previous) =
            state.configs.range(..active).rev().find_map(|(_, record)| {
                (record.document == state.config.document).then_some(record.id)
            })
        {
            format!("configuration {active} re-registered the content of {previous}")
        } else {
            format!("replaced by later configuration {active}")
        };
        ConfigDivergence::Superseded {
            generation: matching.id,
            reason,
        }
    }

    /// Validate new uses outside the committer, then atomically register and publish content.
    pub async fn apply_config(&self, source: ConfigSource) -> Result<Applied, RegistryError> {
        let (view, document, active) = {
            let state = self.state.lock().await;
            let document = match source {
                ConfigSource::Document(document) => document,
                ConfigSource::Generation(id) => state
                    .configs
                    .get(&id)
                    .ok_or_else(|| {
                        if state.plugins.generations.contains_key(&id) {
                            rejected("config", "generation is plugin content")
                        } else {
                            RegistryError::UnknownGeneration(id)
                        }
                    })?
                    .document
                    .clone(),
            };
            (self.view(), document, state.plugins.active_records())
        };
        let previous = view.config.generation;
        if document == view.config.document {
            return Ok(Applied::Unchanged(previous));
        }
        let declaration = declare_valid(&document, &self.owners)?;
        let prepared = registration::settings(
            self.store.as_ref(),
            self.runtime.as_ref(),
            &view.config.plugins,
            &declaration.plugins,
            &active,
            self.clock.now(),
        )
        .await?;
        let mut expected: BTreeMap<_, _> = declaration
            .uses
            .iter()
            .map(|usage| {
                (
                    usage.endpoint.plugin.clone(),
                    view.exports.active_generation(&usage.endpoint.plugin),
                )
            })
            .collect();
        for name in prepared.keys() {
            expected.insert(name.clone(), active.get(name).map(|record| record.id));
        }
        let probes = self
            .probe_new_uses(&view, &declaration.uses, &prepared)
            .await?;
        let mut state = self.state.lock().await;
        if state.config.generation != previous
            || expected.iter().any(|(name, expected)| {
                state
                    .plugins
                    .names
                    .get(name)
                    .and_then(|id| state.plugins.active.get(id))
                    != expected.as_ref()
            })
        {
            return Err(RegistryError::Conflict("config".into()));
        }
        let admission = declaration.admit(&state, &self.host_tool_names, &prepared)?;
        self.check_profiles_in_use(&declaration.kernel).await?;
        let State {
            plugins, configs, ..
        } = &mut *state;
        let record = register(
            self.store.as_ref(),
            configs,
            plugins,
            prepared,
            document,
            self.clock.now(),
        )
        .await?;
        state.config = declaration.into_active(&record);
        self.publish(&mut state, Some(admission));
        Ok(Applied::Registered {
            generation: record.id,
            previous,
            probes,
        })
    }

    /// One real call for each use the active configuration does not already make.
    async fn probe_new_uses(
        &self,
        view: &View,
        uses: &[Use],
        prepared: &BTreeMap<String, registration::Prepared>,
    ) -> Result<Vec<Probe>, RegistryError> {
        let mut probed: Vec<&Use> = Vec::new();
        let mut probes = Vec::new();
        for usage in uses {
            if view
                .config
                .uses
                .iter()
                .chain(probed.iter().copied())
                .any(|previous| previous.same_call(usage))
            {
                continue;
            }
            let (candidate, generation) = match prepared.get(&usage.endpoint.plugin) {
                Some(candidate) => (&candidate.loaded, None),
                None => {
                    let (generation, loaded) = view
                        .exports
                        .loaded(&usage.endpoint.plugin)
                        .map_err(|failure| rejected(&usage.endpoint.plugin, failure.message))?;
                    (loaded, Some(generation))
                }
            };
            let outcome = match probe::call(candidate, usage).await {
                Ok(probed) => {
                    if let Some(generation) = generation {
                        self.report(generation, probed.verdict, None).await?;
                    }
                    probed.outcome
                }
                Err(failure) => Err(failure),
            };
            if let Err(failure) = &outcome {
                return Err(rejected(
                    "config",
                    format!(
                        "{} (endpoint {}): {}: {}",
                        usage.user, usage.endpoint.name, failure.code, failure.message
                    ),
                ));
            }
            probes.push(Probe {
                user: usage.user.clone(),
                endpoint: usage.endpoint.name.clone(),
            });
            probed.push(usage);
        }
        Ok(probes)
    }

    /// A Session's profile is wiring too, so new content must still define every one in use.
    async fn check_profiles_in_use(&self, kernel: &KernelSettings) -> Result<(), RegistryError> {
        let invalid_sessions: Vec<_> = self
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
        if invalid_sessions.is_empty() {
            return Ok(());
        }
        Err(rejected(
            "config",
            format!(
                "profiles are still used by Sessions: {}",
                invalid_sessions.join(", ")
            ),
        ))
    }

    /// Session profile assignment and configuration admission share the same committer.
    pub async fn set_profile(
        &self,
        session: SessionId,
        profile: &str,
    ) -> Result<(), RegistryError> {
        let state = self.state.lock().await;
        if !state.config.kernel.profiles.contains_key(profile) {
            return Err(RegistryError::UnknownProfile(profile.into()));
        }
        self.store.set_profile(session, profile).await?;
        Ok(())
    }

    /// Mechanical recovery publishes registered content even when some uses are unavailable.
    pub async fn rollback_config(&self) -> Result<ConfigGeneration, RegistryError> {
        let mut state = self.state.lock().await;
        let record = state
            .configs
            .range(..state.config.generation)
            .next_back()
            .map(|(_, record)| record.clone())
            .ok_or_else(|| RegistryError::NoRollbackTarget("config".into()))?;
        let declaration = declare(&record.document, &self.owners).map_err(RegistryError::Config)?;
        self.store.activate_config(record.id).await?;
        state.config = declaration.into_active(&record);
        self.publish(&mut state, None);
        Ok(record)
    }
}
