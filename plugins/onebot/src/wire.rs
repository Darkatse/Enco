//! OneBot conversations, locators, request envelopes and delivery evidence; no transport or
//! credentials live here.
use crate::channel::{Fault, Request, Response};
use enco_sdk::{
    channel::{Quote, Quoted},
    types::{Failure, QuoteKind},
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Envelope {
    status: String,
    retcode: i32,
    data: Value,
    wording: Option<String>,
}

/// Who a conversation reaches; its text is `private:<QQ number>` or `group:<group number>`.
#[derive(Clone, Copy)]
pub(super) enum Conversation {
    Private(u64),
    Group(u64),
}

impl Conversation {
    pub(super) fn parse(text: &str) -> Result<Self, Fault> {
        let (scope, id) = text
            .split_once(':')
            .ok_or_else(|| invalid("conversation"))?;
        let id = id.parse().map_err(|_| invalid("conversation ID"))?;
        match scope {
            "private" => Ok(Self::Private(id)),
            "group" => Ok(Self::Group(id)),
            _ => Err(invalid("conversation scope")),
        }
    }
}

impl std::fmt::Display for Conversation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Private(user) => write!(formatter, "private:{user}"),
            Self::Group(group) => write!(formatter, "group:{group}"),
        }
    }
}

/// What a locator handed to the host names; its text is `onebot:image:<URL>`,
/// `onebot:reply:<message ID>` or `onebot:forward:<forward ID>`.
pub(super) enum Locator {
    /// An image, at the address its segment gave.
    Image(String),
    /// The messages a quote shows.
    Quote(Target),
}

/// The messages a reply or merged-forward segment names.
pub(super) enum Target {
    /// The message a reply answers, by its message ID.
    Reply(i32),
    /// A merged forward, by its resource ID.
    Forward(String),
}

impl Locator {
    /// What `text`, a locator this mapper wrote, names.
    pub(super) fn parse(text: &str) -> Result<Self, Fault> {
        let (kind, id) = text
            .strip_prefix("onebot:")
            .and_then(|rest| rest.split_once(':'))
            .ok_or_else(|| invalid("locator"))?;
        match kind {
            "image" => Ok(Self::Image(id.into())),
            "reply" => id
                .parse()
                .map(|id| Self::Quote(Target::Reply(id)))
                .map_err(|_| invalid("reply locator message ID")),
            "forward" => Ok(Self::Quote(Target::Forward(id.into()))),
            _ => Err(invalid("locator kind")),
        }
    }
}

impl std::fmt::Display for Locator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(url) => write!(formatter, "onebot:image:{url}"),
            Self::Quote(Target::Reply(id)) => write!(formatter, "onebot:reply:{id}"),
            Self::Quote(Target::Forward(id)) => write!(formatter, "onebot:forward:{id}"),
        }
    }
}

impl Target {
    /// The quote of these messages, which the host fetches through their locator.
    pub(super) fn quote(self) -> Quote {
        let kind = match self {
            Self::Reply(_) => QuoteKind::Reply,
            Self::Forward(_) => QuoteKind::Forward,
        };
        Quote {
            kind,
            messages: Quoted::Fetch(Locator::Quote(self).to_string()),
        }
    }
}

pub(super) fn invalid(context: &str) -> Fault {
    fault("channel.bad_response", format!("invalid OneBot {context}"))
}

pub(super) fn fault(code: &str, message: String) -> Fault {
    Fault {
        failure: Failure {
            code: code.into(),
            message,
            retryable: false,
        },
        unknown: false,
        fatal: false,
        retry_after: None,
    }
}

/// How long an action without a file may take.
pub(super) const ACTION_TIMEOUT_MS: u32 = 30_000;

pub(super) fn request(action: &str, body: Value, timeout_ms: u32) -> Request {
    Request {
        method: "POST".into(),
        url: format!("/{action}"),
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some(body.to_string().into_bytes()),
        timeout_ms: Some(timeout_ms),
    }
}

fn checked(response: &Response) -> Result<(), Fault> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    Err(Fault {
        failure: Failure {
            code: format!("channel.http.{}", response.status),
            message: format!("OneBot returned HTTP {}", response.status),
            retryable: response.status == 429 || response.status >= 500,
        },
        unknown: !(400..500).contains(&response.status),
        fatal: matches!(response.status, 401 | 403),
        retry_after: None,
    })
}

pub(super) fn answer(response: Response) -> Result<Value, Fault> {
    checked(&response)?;
    let envelope: Envelope = serde_json::from_slice(&response.body).map_err(|_| Fault {
        unknown: true,
        ..invalid("HTTP response envelope")
    })?;
    match envelope.status.as_str() {
        "ok" => Ok(envelope.data),
        "failed" => Err(fault(
            "channel.request_failed",
            format!(
                "OneBot retcode {}: {}",
                envelope.retcode,
                envelope.wording.unwrap_or_default()
            ),
        )),
        _ => Err(Fault {
            unknown: true,
            ..invalid("HTTP response status")
        }),
    }
}
