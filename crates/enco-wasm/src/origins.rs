//! Validated network origins shared by channel destinations and credential recipients.
use enco_core::Failure;
use reqwest::Url;

/// An exact origin or all strict subdomains of one origin.
#[derive(Clone)]
pub(crate) struct Origin {
    url: Url,
    subdomains: bool,
}

impl Origin {
    pub fn parse(declared: &str) -> Result<Self, String> {
        let (scheme, authority) = declared
            .split_once("://")
            .ok_or("origin requires a scheme and host")?;
        let subdomains = authority.starts_with("*.");
        let text = if subdomains {
            format!("{scheme}://{}", &authority[2..])
        } else {
            declared.to_owned()
        };
        let url = Url::parse(&text).map_err(|error| error.to_string())?;
        if declared.trim() != declared
            || declared.contains('\\')
            || !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
            || url.host_str().is_none()
            || url.host_str().is_some_and(|host| host.contains('*'))
            || (subdomains
                && url
                    .host_str()
                    .is_some_and(|host| host.parse::<std::net::IpAddr>().is_ok()))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("origin requires HTTP(S) or WS(S), a host, and no credentials, path, query or fragment".into());
        }
        Ok(Self { url, subdomains })
    }

    pub fn exact(url: &Url) -> Self {
        Self {
            url: url.clone(),
            subdomains: false,
        }
    }

    pub fn matches(&self, destination: &Url) -> bool {
        if self.url.scheme() != destination.scheme()
            || self.url.port_or_known_default() != destination.port_or_known_default()
        {
            return false;
        }
        if self.subdomains {
            self.url
                .host_str()
                .zip(destination.host_str())
                .is_some_and(|(host, destination)| {
                    destination.len() > host.len() + 1
                        && destination.ends_with(host)
                        && destination.as_bytes()[destination.len() - host.len() - 1] == b'.'
                })
        } else {
            self.url.host() == destination.host()
        }
    }
}

/// A connection's selected base URL and the additional origins accepted with its generation.
pub(crate) struct Destinations {
    base: Url,
    declared: Vec<Origin>,
}

impl Destinations {
    pub fn new(base: Url, declared: &[Origin]) -> Self {
        Self {
            base,
            declared: declared.to_vec(),
        }
    }

    pub fn request(&self, requested: &str) -> Result<Url, Failure> {
        let relative = requested.starts_with('/');
        let text = if relative {
            if requested.starts_with("//") {
                return Err(crate::convert::bad(
                    "channel request cannot supply an authority",
                ));
            }
            format!("{}{requested}", self.base.as_str().trim_end_matches('/'))
        } else {
            requested.to_owned()
        };
        self.destination(&text, &["http", "https"])
    }

    pub fn socket(&self, requested: &str) -> Result<Url, Failure> {
        self.destination(requested, &["ws", "wss"])
    }

    fn destination(&self, text: &str, schemes: &[&str]) -> Result<Url, Failure> {
        let url = Url::parse(text)
            .map_err(|error| crate::convert::bad(format!("channel destination: {error}")))?;
        if text.trim() != text
            || text.contains('\\')
            || !schemes.contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(crate::convert::bad(
                "channel destination has an invalid scheme, credentials, fragment or backslash",
            ));
        }
        if url.origin() != self.base.origin()
            && !self.declared.iter().any(|origin| origin.matches(&url))
        {
            return Err(crate::convert::bad(
                "channel destination is outside its declared origins",
            ));
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_only_subdomains_with_the_declared_scheme_and_port() {
        let origin = Origin::parse("wss://*.discord.gg").unwrap();
        for (url, matches) in [
            ("wss://gateway.discord.gg/?v=10", true),
            ("wss://a.gateway.discord.gg/", true),
            ("wss://discord.gg/", false),
            ("wss://evildiscord.gg/", false),
            ("wss://discord.gg.evil.example/", false),
            ("https://gateway.discord.gg/", false),
            ("wss://gateway.discord.gg:444/", false),
        ] {
            assert_eq!(origin.matches(&Url::parse(url).unwrap()), matches, "{url}");
        }
        for invalid in [
            "https://*.127.0.0.1",
            "https://api.example/path",
            "https://user@api.example",
            "https://api.example?x=1",
            "https://*example.com",
            "file://api.example",
        ] {
            assert!(Origin::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn channel_requests_and_sockets_use_the_same_approved_origins() {
        let destinations = Destinations::new(
            Url::parse("https://api.example/v1").unwrap(),
            &[
                Origin::parse("https://cdn.example").unwrap(),
                Origin::parse("wss://*.gateway.example").unwrap(),
            ],
        );
        assert_eq!(
            destinations.request("/messages?x=1").unwrap().as_str(),
            "https://api.example/v1/messages?x=1"
        );
        assert!(
            destinations
                .request("https://cdn.example/image.png")
                .is_ok()
        );
        assert!(
            destinations
                .socket("wss://one.gateway.example/?v=10")
                .is_ok()
        );
        for invalid in [
            "//evil.example/x",
            "https://other.example/x",
            "https://cdn.example/x#fragment",
            "https://user@cdn.example/x",
            "/\\evil.example/x",
        ] {
            assert!(destinations.request(invalid).is_err(), "{invalid}");
        }
        assert!(destinations.socket("https://cdn.example").is_err());
        assert!(destinations.socket("wss://gateway.example").is_err());
    }
}
