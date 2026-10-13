use super::*;
use state::Plugins;

pub(super) fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    }) {
        Ok(())
    } else {
        Err(RegistryError::Rejected {
            name: name.into(),
            reason: "name must use lowercase letters, digits and single hyphens".into(),
        })
    }
}

/// Why a caller cannot use a loaded generation: the interface it imports is missing, or the
/// credential the endpoint names does not match what the generation declares.
pub(super) fn wiring_error(usage: &Use, loaded: &Loaded) -> Option<String> {
    let (exported, interface) = match usage.need {
        Need::Completion { .. } => (loaded.completion.is_some(), "completion"),
        Need::Embedding { .. } => (loaded.embedding.is_some(), "embedding"),
        Need::Decision => (loaded.decision.is_some(), "decision"),
    };
    if !exported {
        return Some(format!("{} requires {interface}", usage.user));
    }
    let mismatch = match (
        loaded.credential,
        usage.endpoint.settings.api_key_env.is_some(),
    ) {
        (None, true) => "sets api_key_env, but the artifact declares no credential",
        (Some(Credential { required: true }), false) => {
            "has no api_key_env, but the artifact requires a credential"
        }
        _ => return None,
    };
    Some(format!("{} {mismatch}", usage.user))
}

pub(super) fn users(uses: &[Use], name: &str) -> Vec<String> {
    uses.iter()
        .filter(|usage| usage.endpoint.plugin == name)
        .map(|usage| usage.user.clone())
        .collect()
}

pub(super) fn admit(uses: &[Use], name: &str, loaded: &Loaded) -> Result<(), RegistryError> {
    let errors: Vec<_> = uses
        .iter()
        .filter(|usage| usage.endpoint.plugin == name)
        .filter_map(|usage| wiring_error(usage, loaded))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(rejected(name, errors.join(", ")))
    }
}

pub(super) fn check_uses(plugins: &Plugins, uses: &[Use]) -> Result<(), RegistryError> {
    for usage in uses {
        let plugin = plugins.names.get(&usage.endpoint.plugin).ok_or_else(|| {
            rejected(
                &usage.endpoint.plugin,
                format!("used by {}; name is not registered", usage.user),
            )
        })?;
        let loaded = plugins
            .active
            .get(plugin)
            .and_then(|id| plugins.loaded.get(id))
            .ok_or_else(|| {
                rejected(
                    &usage.endpoint.plugin,
                    format!("used by {}; no active generation", usage.user),
                )
            })?;
        if let Some(reason) = wiring_error(usage, loaded) {
            return Err(rejected(&usage.endpoint.plugin, reason));
        }
    }
    Ok(())
}
