//! OneBot v11 messages translated without I/O; the host owns the connection and delivery.
mod content;
mod faces;
mod files;
mod incoming;
mod outgoing;
mod quote;
mod wire;

use bindings::exports::enco::plugin::{channel, lifecycle};
use channel::{
    Arrival, Command, Fault, FetchStep, Identity, Outgoing, ReadStep, Reading, Request, Response,
};
use enco_sdk::{
    channel::{ActivityUpdate, ActivityView},
    types::Failure,
};
use serde::Deserialize;
use serde_json::json;

enco_sdk::export!(channel, Plugin);
struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "OneBot v11 messaging through SnowLuma".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "access_token".into(),
                required: true,
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                        name: "Authorization".into(),
                        prefix: "Bearer ".into(),
                    }),
                }],
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

#[derive(Deserialize)]
struct Account {
    user_id: u64,
}

#[derive(Deserialize)]
struct Sent {
    message_id: Option<i32>,
}

impl channel::Guest for Plugin {
    fn origins() -> Vec<String> {
        vec![
            "https://multimedia.nt.qq.com.cn".into(),
            "http://*.qpic.cn".into(),
            "https://*.qpic.cn".into(),
        ]
    }

    fn pacing() -> Option<channel::PacingGap> {
        Some(channel::PacingGap {
            minimum_ms: 1500,
            jitter_ms: 1500,
        })
    }

    fn identify() -> Request {
        wire::request("get_login_info", json!({}), wire::ACTION_TIMEOUT_MS)
    }

    fn identified(response: Response) -> Result<Identity, Fault> {
        let account: Account = serde_json::from_value(wire::answer(response)?)
            .map_err(|_| wire::invalid("get_login_info data.user_id"))?;
        Ok(Identity {
            protocol: "onebot".into(),
            account: account.user_id.to_string(),
            facts: "{}".into(),
        })
    }

    fn read(_identity: Identity, _state: Option<String>) -> Result<ReadStep, Fault> {
        Ok(ReadStep::Socket(channel::Socket {
            url: "/event".into(),
            close_code: 1000,
        }))
    }

    fn received(
        identity: Identity,
        _state: Option<String>,
        arrival: Arrival,
    ) -> Result<Reading, Fault> {
        let messages = match arrival {
            Arrival::Frame(frame) => incoming::message(&frame, &identity.account)?
                .into_iter()
                .collect(),
            Arrival::Closed(_) | Arrival::Timer => vec![],
            Arrival::Response(_) => {
                return Err(wire::invalid("read arrival: expected a WebSocket frame"));
            }
        };
        Ok(Reading {
            messages,
            state: None,
            frames: vec![],
            acknowledgments: vec![],
            timer: None,
            reconnect: false,
        })
    }

    fn fetch(locator: String, responses: Vec<Response>, limit: u32) -> Result<FetchStep, Fault> {
        match wire::Locator::parse(&locator)? {
            wire::Locator::Quote(target) => quote::fetch(target, responses, limit),
            wire::Locator::Download(url) => files::download(url, responses),
            wire::Locator::PrivateFile { file_id, file_hash } => files::fetch(
                "get_private_file_url",
                json!({"file_id": file_id, "file_hash": file_hash}),
                responses,
            ),
            wire::Locator::GroupFile { group_id, file_id } => files::fetch(
                "get_group_file_url",
                json!({"group_id": group_id, "file_id": file_id}),
                responses,
            ),
        }
    }

    fn split(message: Outgoing) -> Vec<Outgoing> {
        outgoing::split(message)
    }

    fn send(conversation: String, part: Outgoing) -> Result<Request, Fault> {
        outgoing::send(&conversation, part)
    }

    fn answered(response: Response) -> Result<Option<String>, Fault> {
        let sent: Option<Sent> =
            serde_json::from_value(wire::answer(response)?).map_err(|_| Fault {
                unknown: true,
                ..wire::invalid("HTTP response data.message_id")
            })?;
        Ok(sent
            .and_then(|sent| sent.message_id)
            .filter(|id| *id != 0)
            .map(|id| id.to_string()))
    }

    fn commands(_identity: Identity, _commands: Vec<Command>) -> Vec<Request> {
        vec![]
    }

    fn respond(_handle: String, _text: Option<String>) -> Result<Option<Request>, Fault> {
        Ok(None)
    }

    // QQ cannot edit a sent message, so a Run has no view to keep current.
    fn activity(
        _conversation: String,
        _replies_here: bool,
        _shown: Option<String>,
        _update: ActivityUpdate,
    ) -> Option<Request> {
        None
    }

    // Only private chats have an input status.
    fn presence(conversation: String, _view: ActivityView) -> Option<Request> {
        let wire::Conversation::Private(user) = wire::Conversation::parse(&conversation).ok()?
        else {
            return None;
        };
        Some(wire::request(
            "set_input_status",
            json!({"user_id": user, "event_type": 1}),
            wire::ACTION_TIMEOUT_MS,
        ))
    }
    fn open_conversation(_beside: String, _session: Vec<String>) -> Option<Request> {
        None
    }

    fn opened(_beside: String, _response: Response) -> Result<String, Fault> {
        Err(wire::fault(
            "plugin.contract",
            "OneBot cannot open child conversations".into(),
        ))
    }

    fn close_conversation(_conversation: String) -> Option<Request> {
        None
    }
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
