//! A generation declares where its credential goes; each invocation attaches the caller's secret.
use crate::bindings::{enco::plugin::host, lifecycle};
use enco_core::{Failure, ProviderSettings, code};
use enco_kernel::LoadError;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};

/// Where a generation places the caller's credential, validated once when the generation loads.
pub(crate) struct Placement {
    header: HeaderName,
    prefix: String,
}

impl Placement {
    pub fn new(credential: &lifecycle::Credential) -> Result<Self, LoadError> {
        let header = HeaderName::from_bytes(credential.header.as_bytes())
            .map_err(|e| LoadError(format!("credential header: {e}")))?;
        HeaderValue::from_str(&credential.prefix)
            .map_err(|e| LoadError(format!("credential prefix: {e}")))?;
        Ok(Self {
            header,
            prefix: credential.prefix.clone(),
        })
    }
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
    /// Attach a credential when the plugin declares a placement and the endpoint names a variable.
    /// Whether the two match is checked when a generation is admitted; a call made after
    /// mechanical recovery simply uses what both sides supply.
    pub fn resolve(
        placement: Option<&Placement>,
        settings: &ProviderSettings,
    ) -> Result<Option<Self>, Failure> {
        let (Some(placement), Some(name)) = (placement, &settings.api_key_env) else {
            return Ok(None);
        };
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
        let origin = Url::parse(&settings.base_url)
            .map_err(|e| failure(code::PROVIDER_BAD_REQUEST, format!("invalid base_url: {e}")))?;
        Ok(Some(Self {
            origin,
            header: placement.header.clone(),
            value,
            secret,
            replacement: format!("[redacted:{name}]"),
        }))
    }

    /// Attach to requests bound for the caller-selected origin, replacing the plugin's header.
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
