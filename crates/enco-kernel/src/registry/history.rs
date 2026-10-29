//! Registration rows are interpreted once, beside the facts that give them meaning.
use super::*;

/// One path's accepted change, named at the registration where it happened.
#[derive(Clone)]
pub enum UnitChange {
    /// An accepted plugin change under its name at this registration.
    Plugin {
        /// Name at the time of the change.
        name: PluginName,
        /// What changed for this plugin.
        change: PluginChangeKind,
    },
    /// Configuration sections changed under their declared ownership.
    Configuration {
        /// Configuration generation selected by this registration.
        generation: GenerationId,
        /// Changed sections at the granularity declared by their owners.
        sections: Vec<String>,
    },
}

/// Accepted changes to one plugin; each variant carries only its own facts.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginChangeKind {
    /// A previously unnamed plugin acquired a name.
    Created,
    /// The accepted name changed while retaining the plugin.
    Renamed {
        /// Name before this registration.
        from: PluginName,
    },
    /// The accepted name was removed.
    Removed,
    /// Source selected new code.
    Code {
        /// Selected generation and its current outcome.
        generation: Box<SelectedGeneration>,
    },
    /// The host's factory code was selected.
    Factory {
        /// Selected generation and its current outcome.
        generation: Box<SelectedGeneration>,
    },
    /// New settings selected a generation using existing code.
    Settings {
        /// Selected generation and its current outcome.
        generation: Box<SelectedGeneration>,
    },
}

/// A generation a registration selected, as it stands now.
#[derive(Clone, serde::Serialize)]
pub struct SelectedGeneration {
    /// The generation and its recorded outcome.
    #[serde(flatten)]
    pub record: GenerationRecord,
    /// Whether its plugin still runs it; a trial generation that does not was never promoted.
    pub active: bool,
}

impl UnitChange {
    /// The public path used by history, status and rewind.
    pub fn path(&self) -> IntentPath {
        match self {
            Self::Plugin { name, .. } => IntentPath::Plugin(name.clone()),
            Self::Configuration { .. } => IntentPath::Configuration,
        }
    }
}

impl PluginChangeKind {
    /// Generation selected by this change, including a reused factory generation.
    pub fn generation(&self) -> Option<&SelectedGeneration> {
        match self {
            Self::Code { generation }
            | Self::Factory { generation }
            | Self::Settings { generation } => Some(generation),
            Self::Created | Self::Renamed { .. } | Self::Removed => None,
        }
    }
}

impl serde::Serialize for UnitChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        #[serde(untagged)]
        enum View<'a> {
            Plugin {
                name: &'a PluginName,
                #[serde(flatten)]
                change: &'a PluginChangeKind,
            },
            Configuration {
                kind: &'static str,
                generation: GenerationId,
                sections: &'a [String],
            },
        }
        match self {
            Self::Plugin { name, change } => View::Plugin { name, change },
            Self::Configuration {
                generation,
                sections,
            } => View::Configuration {
                kind: "configuration",
                generation: *generation,
                sections,
            },
        }
        .serialize(serializer)
    }
}

/// A bounded description of one accepted registration, without unrelated history.
#[derive(serde::Serialize)]
pub struct RegistrationDetails<S = SessionRef> {
    /// Accepted position in the registration history.
    pub id: RegistrationId,
    /// Preceding position, absent for the first registration.
    pub previous: Option<RegistrationId>,
    /// Caller and their explanation.
    pub initiator: Initiator<S>,
    /// Observation time, used only for presentation.
    pub created_at: DateTime<Utc>,
    /// Typed changes derived from the accepted rows.
    pub changes: Vec<UnitChange>,
}

impl Registry {
    /// Read accepted changes newest first, stopping before the supplied cursor.
    pub async fn history(
        &self,
        before: Option<RegistrationId>,
        limit: usize,
    ) -> Result<Vec<RegistrationDetails>, RegistryError> {
        let entries =
            self.state
                .lock()
                .await
                .registration_history(before, limit, &self.deps.owners)?;
        self.resolve_history(entries).await
    }

    /// Read one accepted registration by identity.
    pub async fn registration(
        &self,
        id: RegistrationId,
    ) -> Result<RegistrationDetails, RegistryError> {
        let state = self.state.lock().await;
        let record = state
            .history
            .registrations
            .get(&id)
            .ok_or_else(|| state.unknown_registration(id))?;
        let entry = state.registration_details(record, &self.deps.owners)?;
        drop(state);
        entry.resolve(self.deps.store.as_ref()).await
    }

    pub(super) async fn resolve_history(
        &self,
        entries: Vec<RegistrationDetails<SessionId>>,
    ) -> Result<Vec<RegistrationDetails>, RegistryError> {
        let mut resolved = Vec::with_capacity(entries.len());
        for entry in entries {
            resolved.push(entry.resolve(self.deps.store.as_ref()).await?);
        }
        Ok(resolved)
    }
}

impl RegistrationDetails<SessionId> {
    async fn resolve(self, store: &dyn Store) -> Result<RegistrationDetails, RegistryError> {
        let initiator = match self.initiator {
            Initiator::Owner { reason } => Initiator::Owner { reason },
            Initiator::Host => Initiator::Host,
            Initiator::Agent {
                session,
                call,
                reason,
            } => {
                let record = store
                    .session(session)
                    .await?
                    .ok_or(StoreError::UnknownSession(session))?;
                Initiator::Agent {
                    session: SessionRef {
                        id: session,
                        name: record.name,
                    },
                    call,
                    reason,
                }
            }
        };
        Ok(RegistrationDetails {
            id: self.id,
            previous: self.previous,
            initiator,
            created_at: self.created_at,
            changes: self.changes,
        })
    }
}

impl State {
    pub(super) fn registration_history(
        &self,
        before: Option<RegistrationId>,
        limit: usize,
        owners: &[TableOwner],
    ) -> Result<Vec<RegistrationDetails<SessionId>>, RegistryError> {
        self.history
            .registrations
            .range((
                std::ops::Bound::Unbounded,
                before.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
            ))
            .rev()
            .take(limit)
            .map(|(_, record)| self.registration_details(record, owners))
            .collect()
    }

    pub(super) fn unknown_registration(&self, registration: RegistrationId) -> RegistryError {
        match self.history.registrations.last_key_value() {
            Some((latest, _)) => RegistryError::UnknownRegistration {
                registration,
                latest: *latest,
            },
            None => RegistryError::NoRegistrations,
        }
    }

    fn registration_details(
        &self,
        record: &RegistrationRecord,
        owners: &[TableOwner],
    ) -> Result<RegistrationDetails<SessionId>, RegistryError> {
        let configuration = self
            .history
            .config_generations
            .values()
            .find(|generation| generation.registration == record.id);
        Ok(RegistrationDetails {
            id: record.id,
            previous: self.previous_registration(record.id),
            initiator: record.initiator.clone(),
            created_at: record.created_at,
            changes: self.registration_changes(record, &[], configuration, owners)?,
        })
    }

    fn previous_registration(&self, id: RegistrationId) -> Option<RegistrationId> {
        self.history
            .registrations
            .range(..id)
            .next_back()
            .map(|(id, _)| *id)
    }

    /// Used before committing a candidate and when reading it later; no summary is persisted.
    pub(super) fn registration_changes(
        &self,
        record: &RegistrationRecord,
        pending: &[GenerationRecord],
        configuration: Option<&ConfigGeneration>,
        owners: &[TableOwner],
    ) -> Result<Vec<UnitChange>, RegistryError> {
        let previous = self.previous_registration(record.id);
        let before = previous
            .map(|id| self.registered_names_at(Some(id)))
            .unwrap_or_default();
        let name_before = |id| {
            before
                .iter()
                .find(|(_, existing)| **existing == id)
                .map(|(name, _)| name.clone())
        };
        // A candidate is not in the history yet, and everything it selects becomes active.
        let candidate = !self.history.registrations.contains_key(&record.id);
        let mut changes = Vec::new();
        for (id, row) in &record.names {
            match (&row.name, name_before(*id)) {
                (Some(name), None) => changes.push(UnitChange::Plugin {
                    name: name.clone(),
                    change: PluginChangeKind::Created,
                }),
                (Some(name), Some(from)) if *name != from => changes.push(UnitChange::Plugin {
                    name: name.clone(),
                    change: PluginChangeKind::Renamed { from },
                }),
                (None, Some(name)) => changes.push(UnitChange::Plugin {
                    name,
                    change: PluginChangeKind::Removed,
                }),
                (Some(_), Some(_)) => {}
                (None, None) => {
                    return Err(RegistryError::Intent(format!(
                        "registration {} removes unnamed identity {id}",
                        record.id
                    )));
                }
            }
        }
        for (id, code) in &record.code {
            let name = record
                .names
                .get(id)
                .and_then(|row| row.name.clone())
                .or_else(|| name_before(*id))
                .ok_or_else(|| {
                    RegistryError::Intent(format!(
                        "registration {} selects code for unnamed identity {id}",
                        record.id
                    ))
                })?;
            let generation = pending
                .iter()
                .find(|generation| generation.id == code.generation)
                .or_else(|| self.history.generations.get(&code.generation))
                .ok_or(RegistryError::UnknownGeneration(code.generation))?
                .clone();
            let active = candidate
                || self.history.plugins[id]
                    .active
                    .is_some_and(|active| active.generation == generation.id);
            let generation = Box::new(SelectedGeneration {
                record: generation,
                active,
            });
            let change = match (code.intent, generation.record.origin) {
                (Some(CodeIntent::Directory { .. }), _) => PluginChangeKind::Code { generation },
                (Some(CodeIntent::Factory), _) => PluginChangeKind::Factory { generation },
                (None, Origin::Settings { .. }) => PluginChangeKind::Settings { generation },
                (None, Origin::Code { .. } | Origin::Factory) => {
                    return Err(RegistryError::Intent(format!(
                        "registration {} selects code generation {} without code intent",
                        record.id, code.generation
                    )));
                }
            };
            changes.push(UnitChange::Plugin { name, change });
        }
        if let Some(configuration) = configuration {
            let before = previous
                .and_then(|id| self.registered_config_at(Some(id)))
                .map(|config| &config.document);
            changes.push(UnitChange::Configuration {
                generation: configuration.id,
                sections: configuration::changed_sections(before, &configuration.document, owners),
            });
        }
        Ok(changes)
    }
}
