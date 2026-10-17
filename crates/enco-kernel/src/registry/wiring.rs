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

impl Use {
    /// Availability belongs to this use, not to the generation serving it.
    pub(super) fn check(&self, loaded: &Loaded) -> Result<(), Failure> {
        match self.need {
            Need::Completion { .. } => self
                .endpoint
                .export(loaded, loaded.completion.as_ref(), "completion")
                .map(|_| ()),
            Need::Embedding { .. } => self
                .endpoint
                .export(loaded, loaded.embedding.as_ref(), "embedding")
                .map(|_| ()),
            Need::Decision => self
                .endpoint
                .export(loaded, loaded.decision.as_ref(), "decision")
                .map(|_| ()),
        }
    }

    pub(super) fn same_call(&self, other: &Self) -> bool {
        self.endpoint.plugin == other.endpoint.plugin
            && self.endpoint.settings == other.endpoint.settings
            && self.need == other.need
    }
}
