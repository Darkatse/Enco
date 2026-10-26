use super::*;

impl Endpoint {
    /// Bind this caller's service parameters to one exported interface.
    pub(super) fn export<T>(
        &self,
        loaded: &Loaded,
        adapter: Option<T>,
        interface: &str,
    ) -> Result<T, Failure> {
        let unavailable = |reason: String| Failure {
            code: code::PLUGIN_UNAVAILABLE.into(),
            message: format!("endpoint {} (plugin {}): {reason}", self.name, self.plugin),
            retryable: false,
        };
        let adapter = adapter.ok_or_else(|| unavailable(format!("requires {interface}")))?;
        let mismatch = match (
            loaded
                .credentials
                .iter()
                .find(|credential| credential.origin.is_none()),
            self.settings.api_key_env.is_some(),
        ) {
            (None, true) => Some("sets api_key_env, but the artifact declares no credential"),
            (Some(Credential { required: true, .. }), false) => {
                Some("has no api_key_env, but the artifact requires a credential")
            }
            _ => None,
        };
        match mismatch {
            Some(reason) => Err(unavailable(reason.into())),
            None => Ok(adapter),
        }
    }
}

/// Check and normalize one composer use against the generation that will execute it.
pub(super) fn bind_composer(
    loaded: &Loaded,
    options: &ComposerOptions,
) -> Result<(Arc<dyn Composer>, ComposerOptions), Failure> {
    let unavailable = |message: String| Failure {
        code: code::PLUGIN_UNAVAILABLE.into(),
        message,
        retryable: false,
    };
    let composer = loaded
        .composer
        .as_ref()
        .ok_or_else(|| unavailable("requires composer".into()))?;
    let options = composer
        .schema
        .accept(options.clone())
        .map_err(|error| unavailable(format!("composer options: {error}")))?;
    Ok((composer.adapter.clone(), options))
}

impl Use {
    pub(super) fn plugin(&self) -> &PluginName {
        match &self.target {
            UseTarget::Channel { plugin, .. } => plugin,
            UseTarget::Endpoint { endpoint, .. } => &endpoint.plugin,
            UseTarget::Composer { settings, .. } => &settings.plugin,
        }
    }

    pub(super) fn reference(&self) -> UseRef {
        match &self.target {
            UseTarget::Channel { name, .. } => UseRef::Channel { name: name.clone() },
            UseTarget::Endpoint { endpoint, .. } => UseRef::Endpoint {
                endpoint: endpoint.name.clone(),
            },
            UseTarget::Composer { profile, .. } => UseRef::Composer {
                profile: profile.clone(),
            },
        }
    }

    /// Availability belongs to this use, not to the generation serving it.
    pub(super) fn check(&self, loaded: &Loaded) -> Result<(), Failure> {
        match &self.target {
            UseTarget::Channel { settings, .. } => {
                let unavailable = |message: String| Failure {
                    code: code::PLUGIN_UNAVAILABLE.into(),
                    message,
                    retryable: false,
                };
                if loaded.channel.is_none() {
                    return Err(unavailable("requires channel".into()));
                }
                for credential in &loaded.credentials {
                    if credential.required && !settings.secrets.contains_key(&credential.name) {
                        return Err(unavailable(format!(
                            "missing secret mapping for {}",
                            credential.name
                        )));
                    }
                }
                for name in settings.secrets.keys() {
                    if !loaded
                        .credentials
                        .iter()
                        .any(|credential| &credential.name == name)
                    {
                        return Err(unavailable(format!("undeclared credential {name}")));
                    }
                }
                Ok(())
            }
            UseTarget::Endpoint { endpoint, need } => match need {
                Need::Completion { .. } => endpoint
                    .export(loaded, loaded.completion.as_ref(), "completion")
                    .map(|_| ()),
                Need::Embedding { .. } => endpoint
                    .export(loaded, loaded.embedding.as_ref(), "embedding")
                    .map(|_| ()),
                Need::Decision => endpoint
                    .export(loaded, loaded.decision.as_ref(), "decision")
                    .map(|_| ()),
            },
            UseTarget::Composer { settings, .. } => {
                bind_composer(loaded, &settings.options).map(|_| ())
            }
        }
    }

    pub(super) fn same_call(&self, other: &Self) -> bool {
        match (&self.target, &other.target) {
            (
                UseTarget::Channel {
                    plugin, settings, ..
                },
                UseTarget::Channel {
                    plugin: other,
                    settings: other_settings,
                    ..
                },
            ) => {
                plugin == other
                    && settings.base_url == other_settings.base_url
                    && settings.secrets == other_settings.secrets
            }
            (
                UseTarget::Endpoint { endpoint, need },
                UseTarget::Endpoint {
                    endpoint: other,
                    need: other_need,
                },
            ) => {
                endpoint.plugin == other.plugin
                    && endpoint.settings == other.settings
                    && need == other_need
            }
            (
                UseTarget::Composer { settings, .. },
                UseTarget::Composer {
                    settings: other, ..
                },
            ) => settings == other,
            _ => false,
        }
    }
}
