//! Credentials for plugin HTTP requests.
//!
//! A plugin declares the origin and placement of each credential; the value is read from the
//! environment for each call and never handed to the guest or the kernel.
use crate::bindings::{enco::plugin::host, lifecycle};
use enco_core::{Failure, PluginSettings, ProviderSettings, code};
use enco_kernel::LoadError;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};

/// A credential's destination and placement, validated once when the generation loads.
pub(crate) struct Placement {
    name: String,
    origin: Option<Url>,
    destination: Destination,
}

enum Destination {
    Header { header: HeaderName, prefix: String },
    Path(Vec<String>),
}

impl Placement {
    pub fn new(credential: &lifecycle::Credential) -> Result<Self, LoadError> {
        let name = &credential.name;
        let destination = match &credential.placement {
            lifecycle::Placement::Header(lifecycle::HeaderPlacement {
                name: header,
                prefix,
            }) => {
                let header = HeaderName::from_bytes(header.as_bytes()).map_err(|error| {
                    LoadError::Component(format!("credential {name} header: {error}"))
                })?;
                HeaderValue::from_str(prefix).map_err(|error| {
                    LoadError::Component(format!("credential {name} prefix: {error}"))
                })?;
                Destination::Header {
                    header,
                    prefix: prefix.clone(),
                }
            }
            lifecycle::Placement::Path(prefixes) => {
                if prefixes.is_empty()
                    || prefixes
                        .iter()
                        .any(|prefix| !prefix.starts_with('/') || prefix.contains(['?', '#']))
                {
                    return Err(LoadError::Component(format!(
                        "credential {name} path placement requires nonempty absolute path prefixes without queries or fragments"
                    )));
                }
                Destination::Path(prefixes.clone())
            }
        };
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
            destination,
        })
    }
}

/// Accept declarations once, including the one-credential-per-origin rule.
///
/// Whether the owner's secret mapping fits these declarations is the registry's check, made when
/// it accepts code and settings together.
pub(crate) fn placements(declared: &[lifecycle::Credential]) -> Result<Vec<Placement>, LoadError> {
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

/// Connection credentials follow the service and secret mapping chosen for that account.
pub(crate) fn resolve_channel(
    placements: &[Placement],
    base: &Url,
    secrets: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<CallCredential>, Failure> {
    placements
        .iter()
        .filter_map(|placement| {
            secrets.get(&placement.name).map(|name| {
                let mut credential = CallCredential::resolve(
                    placement,
                    placement.origin.clone().unwrap_or_else(|| base.clone()),
                    name,
                )?;
                if let CallDestination::Path(prefixes) = &mut credential.destination {
                    let base_path = base.path().trim_end_matches('/');
                    for prefix in prefixes {
                        *prefix = format!("{base_path}{prefix}");
                    }
                }
                Ok(credential)
            })
        })
        .collect()
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
    destination: CallDestination,
    secret: String,
    replacement: String,
}

enum CallDestination {
    Header {
        header: HeaderName,
        value: HeaderValue,
    },
    Path(Vec<String>),
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
        let destination = match &placement.destination {
            Destination::Header { header, prefix } => {
                let mut value = HeaderValue::from_str(&format!("{prefix}{secret}"))
                    .map_err(|_| unavailable("not valid in an HTTP header"))?;
                value.set_sensitive(true);
                CallDestination::Header {
                    header: header.clone(),
                    value,
                }
            }
            Destination::Path(prefixes) => CallDestination::Path(prefixes.clone()),
        };
        Ok(Self {
            origin,
            destination,
            secret,
            replacement: format!("[redacted:{name}]"),
        })
    }

    /// Fill the declared position only on requests to this credential's origin.
    pub fn attach(&self, request: &mut reqwest::Request) {
        if request.url().origin() == self.origin.origin() {
            match &self.destination {
                CallDestination::Header { header, value } => {
                    request.headers_mut().insert(header.clone(), value.clone());
                }
                CallDestination::Path(prefixes) => {
                    let path = request.url().path();
                    if let Some(prefix) = prefixes
                        .iter()
                        .filter(|prefix| path.starts_with(prefix.as_str()))
                        .max_by_key(|prefix| prefix.len())
                    {
                        let path = format!("{prefix}{}{}", self.secret, &path[prefix.len()..]);
                        request.url_mut().set_path(&path);
                    }
                }
            }
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
            destination: CallDestination::Header {
                header: HeaderName::from_static("x-api-key"),
                value: HeaderValue::from_static("a-secret"),
            },
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

    #[test]
    fn path_credentials_only_enter_declared_prefixes_at_the_bound_origin() {
        let credential = CallCredential {
            origin: Url::parse("https://api.example").unwrap(),
            destination: CallDestination::Path(vec!["/bot".into(), "/file/bot".into()]),
            secret: "42:test-secret".into(),
            replacement: "[redacted:TOKEN]".into(),
        };
        let client = reqwest::Client::new();
        for (url, expected) in [
            (
                "https://api.example/bot/getMe?x=1",
                "https://api.example/bot42:test-secret/getMe?x=1",
            ),
            (
                "https://api.example/file/bot/images/photo.jpg",
                "https://api.example/file/bot42:test-secret/images/photo.jpg",
            ),
            ("https://api.example/other", "https://api.example/other"),
            (
                "https://other.example/bot/getMe",
                "https://other.example/bot/getMe",
            ),
        ] {
            let mut request = client.get(url).build().unwrap();
            credential.attach(&mut request);
            assert_eq!(request.url().as_str(), expected);
        }
        let mut response = Ok(host::HttpResponse {
            status: 200,
            headers: vec![(
                "location".into(),
                "https://api.example/bot42:test-secret/a".into(),
            )],
            body: b"\xff42:test-secret\x00".to_vec(),
        });
        credential.redact(&mut response);
        let response = response.unwrap();
        assert_eq!(
            response.headers[0].1,
            "https://api.example/bot[redacted:TOKEN]/a"
        );
        assert_eq!(response.body, b"\xff[redacted:TOKEN]\x00");
    }
}
