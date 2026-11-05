use serde::{Deserialize, Serialize};
use std::str::FromStr;
use url::Url;

/// The address of a service: an HTTP(S) origin with an optional base path, which request paths
/// are joined to.
///
/// Only parsing makes one, so it never carries credentials, a query or a fragment; it is kept and
/// compared in its normalized form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BaseUrl(Url);

/// Text that is not a base URL; it does not repeat the text, which may hold a credential.
#[derive(Debug, thiserror::Error)]
pub enum InvalidBaseUrl {
    /// The text is not a URL.
    #[error("base_url is not a URL: {0}")]
    Unparsed(#[from] url::ParseError),
    /// The URL is not an HTTP(S) origin with an optional path.
    #[error("base_url must be an HTTP(S) URL without credentials, query or fragment")]
    Shape,
}

impl BaseUrl {
    /// The parsed URL.
    pub fn url(&self) -> &Url {
        &self.0
    }
}

impl FromStr for BaseUrl {
    type Err = InvalidBaseUrl;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(text)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(InvalidBaseUrl::Shape);
        }
        Ok(Self(url))
    }
}

impl TryFrom<String> for BaseUrl {
    type Error = InvalidBaseUrl;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<BaseUrl> for String {
    fn from(base: BaseUrl) -> Self {
        base.0.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_http_origin_with_a_path_is_a_base_url() {
        for valid in ["https://api.example/v1", "http://127.0.0.1:3000"] {
            assert!(valid.parse::<BaseUrl>().is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "api.example/v1",
            "ftp://api.example/v1",
            "https://user:secret@api.example/v1",
            "https://api.example/v1?key=1",
            "https://api.example/v1#part",
        ] {
            assert!(invalid.parse::<BaseUrl>().is_err(), "{invalid}");
        }
        assert!(serde_json::from_str::<BaseUrl>(r#""ftp://api.example""#).is_err());
    }
}
