//! Validated network origins shared by channel destinations and credential recipients.
use enco_core::Failure;
use reqwest::Url;

/// A declared origin, or the service a caller selects.
#[derive(Clone)]
pub(crate) struct Origin {
    url: Url,
    scope: Scope,
}

/// Which destinations an origin covers; all of them keep its port.
#[derive(Clone, Copy)]
enum Scope {
    /// Its scheme and host.
    Exact,
    /// Its scheme and the strict subdomains of its host.
    Subdomains,
    /// Its host by HTTP or WebSocket with the same transport security, as a caller-selected
    /// service is reached both ways.
    Service,
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
        let scope = if subdomains {
            Scope::Subdomains
        } else {
            Scope::Exact
        };
        Ok(Self { url, scope })
    }

    pub fn service(url: &Url) -> Self {
        Self {
            url: url.clone(),
            scope: Scope::Service,
        }
    }

    pub fn matches(&self, destination: &Url) -> bool {
        let scheme = match self.scope {
            Scope::Service => matches!(
                (self.url.scheme(), destination.scheme()),
                ("http", "http" | "ws") | ("https", "https" | "wss")
            ),
            Scope::Exact | Scope::Subdomains => self.url.scheme() == destination.scheme(),
        };
        if !scheme || self.url.port_or_known_default() != destination.port_or_known_default() {
            return false;
        }
        match self.scope {
            Scope::Subdomains => self.url.host_str().zip(destination.host_str()).is_some_and(
                |(host, destination)| {
                    destination.len() > host.len() + 1
                        && destination.ends_with(host)
                        && destination.as_bytes()[destination.len() - host.len() - 1] == b'.'
                },
            ),
            Scope::Exact | Scope::Service => self.url.host() == destination.host(),
        }
    }
}

/// A connection's selected service and the additional origins accepted with its generation.
pub(crate) struct Destinations {
    service: Origin,
    declared: Vec<Origin>,
}

/// Request schemes, plain then secure.
const REQUEST_SCHEMES: [&str; 2] = ["http", "https"];
/// Socket schemes, plain then secure.
const SOCKET_SCHEMES: [&str; 2] = ["ws", "wss"];

impl Destinations {
    pub fn new(base: Url, declared: &[Origin]) -> Self {
        Self {
            service: Origin::service(&base),
            declared: declared.to_vec(),
        }
    }

    pub fn request(&self, requested: &str) -> Result<Url, Failure> {
        let url = self.resolve(requested, REQUEST_SCHEMES)?;
        within(url, std::iter::once(&self.service).chain(&self.declared))
    }

    pub fn socket(&self, requested: &str) -> Result<Url, Failure> {
        let url = self.resolve(requested, SOCKET_SCHEMES)?;
        within(url, std::iter::once(&self.service).chain(&self.declared))
    }

    /// An absolute URL taken from external content, at a declared origin only: the service is
    /// reached by paths the mapper builds, never by one content chooses.
    pub fn download(&self, requested: &str) -> Result<Url, Failure> {
        if requested.starts_with('/') {
            return Err(crate::convert::bad("a download needs an absolute URL"));
        }
        within(self.resolve(requested, REQUEST_SCHEMES)?, &self.declared)
    }

    /// `requested` as an absolute URL in one of `schemes`, or as a path joined to the service
    /// with the scheme of the service's transport security.
    fn resolve(&self, requested: &str, schemes: [&str; 2]) -> Result<Url, Failure> {
        let text = if requested.starts_with('/') {
            if requested.starts_with("//") {
                return Err(crate::convert::bad(
                    "channel destination cannot supply an authority",
                ));
            }
            let base = &self.service.url;
            format!(
                "{}{}{requested}",
                schemes[usize::from(base.scheme() == "https")],
                base.as_str()[base.scheme().len()..].trim_end_matches('/')
            )
        } else {
            requested.to_owned()
        };
        let url = Url::parse(&text)
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
        Ok(url)
    }
}

fn within<'a>(url: Url, origins: impl IntoIterator<Item = &'a Origin>) -> Result<Url, Failure> {
    if origins.into_iter().any(|origin| origin.matches(&url)) {
        Ok(url)
    } else {
        Err(crate::convert::bad(
            "channel destination is outside its declared origins",
        ))
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
    fn a_selected_service_covers_its_websocket_with_the_same_transport_security() {
        let service = Origin::service(&Url::parse("https://api.example/v1").unwrap());
        for (url, matches) in [
            ("https://api.example/send", true),
            ("wss://api.example/event", true),
            ("ws://api.example/event", false),
            ("wss://api.example:444/event", false),
            ("wss://cdn.example/event", false),
        ] {
            assert_eq!(service.matches(&Url::parse(url).unwrap()), matches, "{url}");
        }
        let declared = Origin::parse("https://cdn.example").unwrap();
        assert!(!declared.matches(&Url::parse("wss://cdn.example/event").unwrap()));
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
        assert!(destinations.socket("wss://cdn.example").is_err());
    }

    #[test]
    fn selected_service_sockets_keep_the_base_path_and_transport_security() {
        for (base, expected) in [
            ("http://127.0.0.1:3000", "ws://127.0.0.1:3000/event"),
            ("https://api.example/v1/", "wss://api.example/v1/event"),
            ("http://api.example:443", "ws://api.example:443/event"),
        ] {
            let destinations = Destinations::new(Url::parse(base).unwrap(), &[]);
            assert_eq!(destinations.socket("/event").unwrap().as_str(), expected);
            assert!(destinations.socket(expected).is_ok());
        }
        let destinations = Destinations::new(Url::parse("https://api.example").unwrap(), &[]);
        for (invalid, because) in [
            (
                "ws://api.example/event",
                "drops the service's transport security",
            ),
            ("wss://api.example:444/event", "uses another port"),
            ("wss://other.example/event", "names another host"),
            ("/event#fragment", "carries a fragment"),
        ] {
            assert!(destinations.socket(invalid).is_err(), "{invalid} {because}");
        }
    }
}
