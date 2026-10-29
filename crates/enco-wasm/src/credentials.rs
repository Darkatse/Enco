//! Credentials stay in the transport; validated placements bind each recipient and location.
use crate::{bindings::enco::plugin::host, origins::Origin};
use enco_core::{Failure, PluginSettings, ProviderSettings, code};
use enco_kernel::{LoadError, Location};
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};

/// One secret mapping and every validated place it may be written.
pub(crate) struct Credential {
    name: String,
    caller_selected: bool,
    placements: Vec<Placement>,
}

struct Placement {
    origin: Option<Origin>,
    destination: Destination,
}

enum Destination {
    Header { header: HeaderName, prefix: String },
    Path(Vec<String>),
    Frame(String),
}

impl Placement {
    fn new(name: &str, placement: &enco_kernel::Placement) -> Result<Self, LoadError> {
        let destination = match &placement.location {
            Location::Header(enco_kernel::HeaderPlacement {
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
            Location::Path(prefixes) => {
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
            Location::Frame(pointer) => {
                if !valid_pointer(pointer) {
                    return Err(LoadError::Component(format!(
                        "credential {name} frame placement requires a JSON pointer"
                    )));
                }
                Destination::Frame(pointer.clone())
            }
        };
        let origin = placement
            .origin
            .as_deref()
            .map(|declared| {
                Origin::parse(declared).map_err(|error| {
                    LoadError::Component(format!("credential {name} origin {declared}: {error}"))
                })
            })
            .transpose()?;
        Ok(Self {
            origin,
            destination,
        })
    }
}

/// Accept credential names and all placements at the declaration boundary.
pub(crate) fn declarations(
    declared: &[enco_kernel::Credential],
) -> Result<Vec<Credential>, LoadError> {
    let mut names = std::collections::HashSet::new();
    declared
        .iter()
        .map(|credential| {
            if credential.name.is_empty() || !names.insert(&credential.name) {
                return Err(LoadError::Component(format!(
                    "empty or duplicate credential name {}",
                    credential.name
                )));
            }
            if credential.placements.is_empty() {
                return Err(LoadError::Component(format!(
                    "credential {} requires at least one placement",
                    credential.name
                )));
            }
            Ok(Credential {
                name: credential.name.clone(),
                caller_selected: credential.caller_selected(),
                placements: credential
                    .placements
                    .iter()
                    .map(|placement| Placement::new(&credential.name, placement))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(character) = chars.next() {
        if character == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

/// A caller-selected secret also fills that credential's fixed-origin placements.
pub(crate) fn resolve(
    declared: &[Credential],
    settings: &PluginSettings,
    endpoint: Option<&ProviderSettings>,
) -> Result<Vec<CallCredential>, Failure> {
    let mut credentials = Vec::new();
    for credential in declared {
        let (base, name) = if credential.caller_selected {
            let Some(endpoint) = endpoint else { continue };
            (
                Some(service_url(&endpoint.base_url)?),
                endpoint.api_key_env.as_ref(),
            )
        } else {
            (None, settings.secrets.get(&credential.name))
        };
        if let Some(name) = name {
            credentials.push(CallCredential::resolve(
                credential,
                base.as_ref(),
                name,
                "",
            )?);
        }
    }
    Ok(credentials)
}

/// Connection credentials follow the service and secret mapping chosen for that account.
pub(crate) fn resolve_channel(
    declared: &[Credential],
    base: &Url,
    secrets: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<CallCredential>, Failure> {
    declared
        .iter()
        .filter_map(|credential| {
            secrets.get(&credential.name).map(|name| {
                CallCredential::resolve(
                    credential,
                    Some(base),
                    name,
                    base.path().trim_end_matches('/'),
                )
            })
        })
        .collect()
}

fn service_url(url: &str) -> Result<Url, Failure> {
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

/// One invocation's credential, shared by all its placements and never handed to the guest.
pub(crate) struct CallCredential {
    placements: Vec<CallPlacement>,
    secret: String,
    replacement: String,
}

struct CallPlacement {
    origin: Origin,
    destination: CallDestination,
}

enum CallDestination {
    Header {
        header: HeaderName,
        value: HeaderValue,
    },
    Path(Vec<String>),
    Frame(String),
}

impl CallCredential {
    fn resolve(
        credential: &Credential,
        base: Option<&Url>,
        name: &str,
        path_prefix: &str,
    ) -> Result<Self, Failure> {
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
        let placements = credential
            .placements
            .iter()
            .map(|placement| {
                let origin = match &placement.origin {
                    Some(origin) => origin.clone(),
                    None => Origin::service(base.ok_or_else(|| {
                        failure(
                            code::PLUGIN_CONTRACT,
                            "caller-selected credential requires a service URL",
                        )
                    })?),
                };
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
                    Destination::Path(prefixes) => {
                        let prefix = if placement.origin.is_none() {
                            path_prefix
                        } else {
                            ""
                        };
                        CallDestination::Path(
                            prefixes
                                .iter()
                                .map(|path| format!("{prefix}{path}"))
                                .collect(),
                        )
                    }
                    Destination::Frame(pointer) => CallDestination::Frame(pointer.clone()),
                };
                Ok(CallPlacement {
                    origin,
                    destination,
                })
            })
            .collect::<Result<_, Failure>>()?;
        Ok(Self {
            placements,
            secret,
            replacement: format!("[redacted:{name}]"),
        })
    }

    /// Fill declared positions only on requests to their credential recipients.
    pub fn attach(&self, request: &mut reqwest::Request) {
        for placement in &self.placements {
            if !placement.origin.matches(request.url()) {
                continue;
            }
            match &placement.destination {
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
                CallDestination::Frame(_) => {}
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

    pub(crate) fn redact_text(&self, text: &mut String) {
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

/// Remove every resolved credential from text returned by the transport.
pub(crate) fn redact_text(credentials: &[CallCredential], text: &str) -> String {
    let mut text = text.to_owned();
    for credential in credentials {
        credential.redact_text(&mut text);
    }
    text
}

/// Fill null frame positions only for credentials bound to this socket's origin.
pub(crate) fn attach_frame(
    credentials: &[CallCredential],
    destination: &Url,
    frame: &str,
) -> Result<String, Failure> {
    let mut placements = credentials
        .iter()
        .flat_map(|credential| {
            credential.placements.iter().filter_map(move |placement| {
                if placement.origin.matches(destination)
                    && let CallDestination::Frame(pointer) = &placement.destination
                {
                    Some((credential, pointer))
                } else {
                    None
                }
            })
        })
        .peekable();
    if placements.peek().is_none() {
        return Ok(frame.to_owned());
    }
    let mut value: serde_json::Value = serde_json::from_str(frame).map_err(|_| {
        failure(
            code::PLUGIN_CONTRACT,
            "credential-bearing frame must be JSON",
        )
    })?;
    for (credential, pointer) in placements {
        if let Some(position) = value.pointer_mut(pointer) {
            if !position.is_null() {
                return Err(failure(
                    code::PLUGIN_CONTRACT,
                    format!("credential frame position {pointer} must be null"),
                ));
            }
            *position = serde_json::Value::String(credential.secret.clone());
        }
    }
    Ok(value.to_string())
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
