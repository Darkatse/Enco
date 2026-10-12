use super::*;
use crate::{
    ProviderRequest, Question, QuestionKind,
    limits::{
        CONFIG_PROBE_MAX_TOKENS, CONFIG_PROBE_PROMPT, CONFIG_PROBE_QUESTION, CONFIG_PROBE_TOOL,
    },
};

pub(super) struct Declaration {
    pub kernel: KernelSettings,
    pub uses: Vec<Use>,
    pub invalid: Vec<SettingsError>,
}

/// Ownership is a property of the composition, independent of any document.
pub(super) fn check_owners(owners: &[TableOwner]) -> Result<(), RegistryError> {
    let mut tables = std::collections::BTreeSet::from(KernelSettings::TABLES);
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
            !KernelSettings::TABLES.contains(&table.as_str())
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
    /// The only way to activate content: with the declarations evaluated from it.
    pub fn into_active(self, record: &ConfigGeneration) -> ActiveConfig {
        ActiveConfig {
            generation: record.id,
            document: record.document.clone(),
            kernel: self.kernel,
            uses: self.uses,
            invalid: self.invalid,
        }
    }
}

/// Number and persist new content, keeping the in-memory history in step with the Store.
pub(super) async fn register(
    store: &dyn Store,
    configs: &mut BTreeMap<GenerationId, ConfigGeneration>,
    document: serde_json::Value,
    created_at: DateTime<Utc>,
) -> Result<ConfigGeneration, RegistryError> {
    let id = store
        .insert_config_generation(&document, created_at)
        .await?;
    let record = ConfigGeneration {
        id,
        document,
        created_at,
    };
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
            return ConfigDivergence::None;
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
        let (view, document) = {
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
            (self.view(), document)
        };
        let previous = view.config.generation;
        if document == view.config.document {
            return Ok(Applied::Unchanged(previous));
        }
        let declaration = declare_valid(&document, &self.owners)?;
        let expected: BTreeMap<_, _> = declaration
            .uses
            .iter()
            .map(|usage| {
                (
                    usage.endpoint.plugin.clone(),
                    view.exports
                        .plugins
                        .get(&usage.endpoint.plugin)
                        .and_then(|routes| routes.active.as_ref())
                        .map(|loaded| loaded.generation),
                )
            })
            .collect();
        let mut probed: Vec<&Use> = Vec::new();
        let mut probes = Vec::new();
        for usage in &declaration.uses {
            if view
                .config
                .uses
                .iter()
                .chain(probed.iter().copied())
                .any(|previous| same_call(previous, usage))
            {
                continue;
            }
            let outcome = self.probe(&view.exports, usage).await?;
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
        check_uses(&state.plugins, &declaration.uses)?;
        let invalid_sessions: Vec<_> = self
            .store
            .sessions()
            .await?
            .into_iter()
            .filter(|session| !declaration.kernel.profiles.contains_key(&session.profile))
            .map(|session| {
                format!(
                    "{} ({}, profile {})",
                    session.name, session.id, session.profile
                )
            })
            .collect();
        if !invalid_sessions.is_empty() {
            return Err(rejected(
                "config",
                format!(
                    "profiles are still used by Sessions: {}",
                    invalid_sessions.join(", ")
                ),
            ));
        }
        let record = register(
            self.store.as_ref(),
            &mut state.configs,
            document,
            self.clock.now(),
        )
        .await?;
        state.config = declaration.into_active(&record);
        self.publish(&mut state);
        Ok(Applied::Registered {
            generation: record.id,
            previous,
            probes,
        })
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

    /// Mechanical recovery uses registered content without probes or admission.
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
        self.publish(&mut state);
        Ok(record)
    }

    async fn probe(
        &self,
        exports: &Exports,
        usage: &Use,
    ) -> Result<Result<(), Failure>, RegistryError> {
        let (generation, outcome) = match usage.need {
            Need::Completion { tools } => {
                let export = match exports.completion(&usage.endpoint.plugin, false) {
                    Ok(export) => export,
                    Err(failure) => return Ok(Err(failure)),
                };
                let request = ProviderRequest {
                    messages: vec![Message::text(Role::User, CONFIG_PROBE_PROMPT)],
                    tools: if tools {
                        vec![CONFIG_PROBE_TOOL.clone()]
                    } else {
                        vec![]
                    },
                    max_output_tokens: Some(CONFIG_PROBE_MAX_TOKENS),
                };
                (
                    export.generation,
                    export
                        .adapter
                        .complete(&usage.endpoint.settings, request)
                        .await
                        .map(|_| ()),
                )
            }
            Need::Embedding { dimensions } => {
                let export = match exports.embedding(&usage.endpoint.plugin) {
                    Ok(export) => export,
                    Err(failure) => return Ok(Err(failure)),
                };
                let outcome = export
                    .adapter
                    .embed(&usage.endpoint.settings, vec![CONFIG_PROBE_PROMPT.into()])
                    .await;
                self.report(export.generation, verdict(&outcome), None)
                    .await?;
                // Width is the caller's requirement, not a plugin health failure.
                return Ok(outcome.and_then(|vectors| {
                    if vectors.len() == 1 && vectors[0].len() == dimensions.get() {
                        Ok(())
                    } else {
                        Err(Failure {
                            code: code::PROVIDER_BAD_RESPONSE.into(),
                            message: format!("expected one embedding of width {dimensions}"),
                            retryable: false,
                        })
                    }
                }));
            }
            Need::Decision => {
                let export = match exports.decision(&usage.endpoint.plugin) {
                    Ok(export) => export,
                    Err(failure) => return Ok(Err(failure)),
                };
                let questions = vec![Question {
                    instructions: CONFIG_PROBE_QUESTION.into(),
                    kind: QuestionKind::Predicate,
                }];
                (
                    export.generation,
                    export
                        .adapter
                        .decide(
                            &usage.endpoint.settings,
                            CONFIG_PROBE_PROMPT.into(),
                            questions,
                        )
                        .await
                        .map(|_| ()),
                )
            }
        };
        self.report(generation, verdict(&outcome), None).await?;
        Ok(outcome)
    }
}

fn same_call(left: &Use, right: &Use) -> bool {
    left.endpoint.plugin == right.endpoint.plugin
        && left.endpoint.settings == right.endpoint.settings
        && left.need == right.need
}

fn verdict<T>(outcome: &Result<T, Failure>) -> Verdict {
    match outcome {
        Ok(_) => Verdict::Ok,
        Err(failure) => Verdict::Failed(failure.clone()),
    }
}
