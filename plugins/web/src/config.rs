use crate::{
    bad,
    bindings::exports::enco::plugin::lifecycle::{Credential, HeaderPlacement, Placement},
    types::Failure,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Engine {
    Exa,
    Tinyfish,
}

impl Engine {
    pub fn service(self) -> Service {
        match self {
            Self::Exa => Service::Exa,
            Self::Tinyfish => Service::Tinyfish,
        }
    }
    pub fn name(self) -> &'static str {
        self.service().name()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Service {
    Jina,
    Exa,
    Tinyfish,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Self::Jina => "jina",
            Self::Exa => "exa",
            Self::Tinyfish => "tinyfish",
        }
    }

    fn defaults(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Jina => ("https://r.jina.ai", "authorization", "Bearer "),
            Self::Exa => ("https://api.exa.ai", "x-api-key", ""),
            Self::Tinyfish => ("https://api.search.tinyfish.ai", "x-api-key", ""),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub search: Vec<Engine>,
    urls: BTreeMap<Service, String>,
}

impl Config {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        let config: Self =
            serde_json::from_str(input).map_err(|error| bad(format!("configuration: {error}")))?;
        for (index, engine) in config.search.iter().enumerate() {
            if config.search[..index].contains(engine) {
                return Err(bad("search engines must be unique"));
            }
        }
        for (service, address) in &config.urls {
            let service = service.name();
            let url = url::Url::parse(address)
                .map_err(|error| bad(format!("urls.{service}: {error}")))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(bad(format!(
                    "urls.{service} must be an HTTP base URL without credentials, query or fragment"
                )));
            }
        }
        Ok(config)
    }

    #[expect(
        clippy::expect_used,
        reason = "the host calls tools only with configuration accepted by describe; a violated contract must trap"
    )]
    pub fn accepted(input: &str) -> Self {
        serde_json::from_str(input).expect("configuration accepted by describe")
    }

    pub fn url(&self, service: Service) -> &str {
        self.urls
            .get(&service)
            .map(String::as_str)
            .unwrap_or(service.defaults().0)
            .trim_end_matches('/')
    }

    pub fn credentials(&self) -> Vec<Credential> {
        std::iter::once((Service::Jina, false))
            .chain(self.search.iter().map(|engine| (engine.service(), true)))
            .map(|(service, required)| {
                let (_, header, prefix) = service.defaults();
                Credential {
                    name: service.name().into(),
                    origin: Some(self.url(service).into()),
                    required,
                    placement: Placement::Header(HeaderPlacement {
                        name: header.into(),
                        prefix: prefix.into(),
                    }),
                }
            })
            .collect()
    }
}

pub(crate) fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "search": {"type": "array", "items": {"enum": ["exa", "tinyfish"]}, "uniqueItems": true, "description": "Enabled search engines in default-first order. Each requires its named secret mapping."},
            "urls": {"type": "object", "additionalProperties": false, "properties": {
                "jina": {"type": "string", "description": "Jina Reader base URL."},
                "exa": {"type": "string", "description": "Exa base URL."},
                "tinyfish": {"type": "string", "description": "TinyFish Search base URL."}
            }}
        }
    })
}
