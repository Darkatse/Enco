//! Credentials for plugin HTTP requests.
//!
//! A plugin declares the origin and header for each credential; the value is read from the
//! environment for each call and never handed to the guest or the kernel.
use crate::bindings::{enco::plugin::host, lifecycle};
use enco_core::{Failure, PluginSettings, ProviderSettings, code};
use enco_kernel::LoadError;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};

/// A credential's destination and header, validated once when the generation loads.
pub(crate) struct Placement {
    name: String,
    origin: Option<Url>,
    required: bool,
    header: HeaderName,
    prefix: String,
}

impl Placement {
    pub fn new(credential: &lifecycle::Credential) -> Result<Self, LoadError> {
        let name = &credential.name;
        let header = HeaderName::from_bytes(credential.header.as_bytes()).map_err(|e| {
            LoadError::Component(format!(
                "credential {name} header {}: {e}",
                credential.header
            ))
        })?;
        HeaderValue::from_str(&credential.prefix)
            .map_err(|e| LoadError::Component(format!("credential {name} prefix: {e}")))?;
        Ok(Self {
            name: name.clone(),
            origin: credential
                .origin
                .as_deref()
                .map(|declared| {
                    origin(declared).map_err(|error| {
                        LoadError::Component(format!(
                            "credential {name} origin {declared}: {}",
                            error.message
                        ))
                    })
                })
                .transpose()?,
            required: credential.required,
            header,
            prefix: credential.prefix.clone(),
        })
    }
}

/// Accept declarations once, including the one-credential-per-origin rule.
pub(crate) fn placements(
    declared: &[lifecycle::Credential],
    settings: &PluginSettings,
) -> Result<Vec<Placement>, LoadError> {
    let mut names = std::collections::HashSet::new();
    let mut origins = std::collections::HashMap::new();
    let mut placements = Vec::new();
    for credential in declared {
        if credential.name.is_empty() {
            return Err(LoadError::Component(format!(
                "credential name is empty (origin {})",
                origin_text(credential.origin.as_deref())
            )));
        }
        if !names.insert(&credential.name) {
            return Err(LoadError::Component(format!(
                "duplicate credential name {}",
                credential.name
            )));
        }
        let placement = Placement::new(credential)?;
        if let Some(previous) =
            origins.insert(placement.origin.as_ref().map(Url::origin), &credential.name)
        {
            let origin = placement
                .origin
                .as_ref()
                .map(|url| url.origin().ascii_serialization());
            return Err(LoadError::Component(format!(
                "credentials {previous} and {} both bind origin {}",
                credential.name,
                origin_text(origin.as_deref())
            )));
        }
        placements.push(placement);
    }
    for name in settings.secrets.keys() {
        if !placements
            .iter()
            .any(|placement| placement.name == *name && placement.origin.is_some())
        {
            return Err(LoadError::Settings(format!(
                "secret {name} has no declared plugin-owned origin"
            )));
        }
    }
    for placement in &placements {
        if placement.origin.is_some()
            && placement.required
            && !settings.secrets.contains_key(&placement.name)
        {
            return Err(LoadError::Settings(format!(
                "required credential {} has no secret mapping",
                placement.name
            )));
        }
    }
    Ok(placements)
}

/// Values live only for an invocation. A fixed origin reads the generation's mapping; an
/// unbound declaration reads the endpoint selected by its caller.
pub(crate) fn resolve(
    placements: &[Placement],
    settings: &PluginSettings,
    endpoint: Option<&ProviderSettings>,
) -> Result<Vec<CallCredential>, Failure> {
    let mut credentials = Vec::new();
    for placement in placements {
        let (origin, name) = match &placement.origin {
            Some(origin) => (origin.clone(), settings.secrets.get(&placement.name)),
            None => {
                let Some(endpoint) = endpoint else { continue };
                (origin(&endpoint.base_url)?, endpoint.api_key_env.as_ref())
            }
        };
        if let Some(name) = name {
            credentials.push(CallCredential::resolve(placement, origin, name)?);
        }
    }
    Ok(credentials)
}

/// How load errors name an origin; an absent one follows the endpoint its caller selects.
fn origin_text(origin: Option<&str>) -> &str {
    origin.unwrap_or("caller-selected service")
}

fn origin(url: &str) -> Result<Url, Failure> {
    let parsed = Url::parse(url).map_err(|error| {
        failure(
            code::PROVIDER_BAD_REQUEST,
            format!("invalid service URL: {error}"),
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(failure(
            code::PROVIDER_BAD_REQUEST,
            "service URL requires an HTTP origin",
        ));
    }
    Ok(parsed)
}

/// One invocation's credential. It exists only in the call's transport state, never in the guest
/// or the kernel.
pub(crate) struct CallCredential {
    origin: Url,
    header: HeaderName,
    value: HeaderValue,
    secret: String,
    replacement: String,
}

impl CallCredential {
    fn resolve(placement: &Placement, origin: Url, name: &str) -> Result<Self, Failure> {
        let unavailable = |reason: &str| {
            failure(
                code::CREDENTIAL_UNAVAILABLE,
                format!("credential environment variable {name}: {reason}"),
            )
        };
        let secret = std::env::var(name).map_err(|_| unavailable("not set or not valid UTF-8"))?;
        // Redaction searches for the value, so an empty one could never be redacted.
        if secret.is_empty() {
            return Err(unavailable("empty value"));
        }
        let mut value = HeaderValue::from_str(&format!("{}{secret}", placement.prefix))
            .map_err(|_| unavailable("not valid in an HTTP header"))?;
        value.set_sensitive(true);
        Ok(Self {
            origin,
            header: placement.header.clone(),
            value,
            secret,
            replacement: format!("[redacted:{name}]"),
        })
    }

    /// Attach to requests bound for this credential's origin, replacing the plugin's header.
    pub fn attach(&self, request: &mut reqwest::Request) {
        if request.url().origin() == self.origin.origin() {
            request
                .headers_mut()
                .insert(self.header.clone(), self.value.clone());
        }
    }

    /// Replace the secret in everything returned to the plugin.
    pub fn redact(&self, result: &mut Result<host::HttpResponse, host::HttpFailure>) {
        match result {
            Ok(response) => {
                for (_, value) in &mut response.headers {
                    self.redact_text(value);
                }
                self.redact_body(&mut response.body);
            }
            Err(failure) => self.redact_text(&mut failure.message),
        }
    }

    fn redact_text(&self, text: &mut String) {
        if text.contains(&self.secret) {
            *text = text.replace(&self.secret, &self.replacement);
        }
    }

    fn redact_body(&self, body: &mut Vec<u8>) {
        let secret = self.secret.as_bytes();
        let mut rest = body.as_slice();
        let mut redacted = Vec::new();
        while let Some(start) = rest.windows(secret.len()).position(|part| part == secret) {
            redacted.extend_from_slice(&rest[..start]);
            redacted.extend_from_slice(self.replacement.as_bytes());
            rest = &rest[start + secret.len()..];
        }
        if !redacted.is_empty() {
            redacted.extend_from_slice(rest);
            *body = redacted;
        }
    }
}

fn failure(code: &str, message: impl Into<String>) -> Failure {
    Failure {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_origin_scoped_and_redaction_preserves_binary_data() {
        let credential = CallCredential {
            origin: Url::parse("https://api.example:443/v1").unwrap(),
            header: HeaderName::from_static("x-api-key"),
            value: HeaderValue::from_static("a-secret"),
            secret: "a-secret".into(),
            replacement: "[redacted:KEY]".into(),
        };
        let client = reqwest::Client::new();
        for (url, allowed) in [
            ("https://api.example/next", true),
            ("http://api.example/next", false),
            ("https://api.example:444/next", false),
            ("https://other.example/next", false),
        ] {
            let mut request = client
                .get(url)
                .header("x-api-key", "guest-value")
                .build()
                .unwrap();
            credential.attach(&mut request);
            assert_eq!(
                request.headers()["x-api-key"],
                if allowed { "a-secret" } else { "guest-value" }
            );
        }
        let mut body = b"\xffa-secret\x00a-secret\xfe".to_vec();
        credential.redact_body(&mut body);
        assert_eq!(body, b"\xff[redacted:KEY]\x00[redacted:KEY]\xfe");
    }
}
