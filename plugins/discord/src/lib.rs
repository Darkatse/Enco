//! Discord Gateway and Components V2 translation; the host owns all I/O and state.
mod content;
mod gateway;
mod incoming;
mod outgoing;
mod wire;

use bindings::exports::enco::plugin::{channel, lifecycle};
use channel::{
    Arrival, Command, Fault, FetchStep, HttpRequest, HttpResponse, Identity, Outgoing, ReadStep,
    Reading,
};
use enco_sdk::{
    channel::{ActivityUpdate, ActivityView},
    types::Failure,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

enco_sdk::export!(channel, Plugin);
struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "Discord Gateway messaging with Components V2".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "token".into(),
                required: true,
                placements: vec![
                    lifecycle::Placement {
                        origin: None,
                        location: lifecycle::Location::Header(lifecycle::HeaderPlacement {
                            name: "authorization".into(),
                            prefix: "Bot ".into(),
                        }),
                    },
                    lifecycle::Placement {
                        origin: Some("wss://*.discord.gg".into()),
                        location: lifecycle::Location::Frame("/d/token".into()),
                    },
                ],
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

/// The most characters Discord allows in a thread's name.
const THREAD_NAME_CHARS: usize = 100;

#[derive(Deserialize)]
struct Application {
    id: String,
    bot: incoming::User,
}

#[derive(Deserialize, Serialize)]
struct Facts {
    application: String,
}

impl channel::Guest for Plugin {
    fn origins() -> Vec<String> {
        ["wss://*.discord.gg", "https://cdn.discordapp.com"]
            .map(String::from)
            .to_vec()
    }

    fn pacing() -> Option<channel::PacingGap> {
        None
    }

    fn identify() -> HttpRequest {
        wire::request("GET", "/applications/@me".into(), None)
    }

    fn identified(response: HttpResponse) -> Result<Identity, Fault> {
        let application: Application = wire::answer(response)?;
        let facts = Facts {
            application: application.id,
        };
        Ok(Identity {
            protocol: "discord".into(),
            account: application.bot.id,
            facts: json!(facts).to_string(),
        })
    }

    fn read(_identity: Identity, state: Option<String>) -> Result<ReadStep, Fault> {
        gateway::read(state.as_deref())
    }

    fn received(
        identity: Identity,
        state: Option<String>,
        arrival: Arrival,
    ) -> Result<Reading, Fault> {
        gateway::received(identity, state.as_deref(), arrival)
    }

    fn fetch(
        locator: String,
        responses: Vec<HttpResponse>,
        _limit: u32,
    ) -> Result<FetchStep, Fault> {
        let mut responses = responses.into_iter();
        if let Some(channel) = locator.strip_prefix(incoming::CHANNEL_NAME) {
            let Some(response) = responses.next() else {
                return Ok(FetchStep::Request(wire::request(
                    "GET",
                    format!("/channels/{}", wire::path(channel)),
                    None,
                )));
            };
            // Only a server's channels and threads are named this way, and each has a name.
            #[derive(Deserialize)]
            struct Channel {
                name: String,
            }
            return match wire::answer::<Channel>(response) {
                Ok(channel) => Ok(FetchStep::Name(channel.name)),
                Err(error) if !error.fatal && !error.unknown && !error.failure.retryable => {
                    Ok(FetchStep::Unavailable(error.failure.message))
                }
                Err(error) => Err(error),
            };
        }
        let Some(response) = responses.next() else {
            return Ok(FetchStep::Download(locator));
        };
        match wire::checked(&response) {
            Ok(()) => Ok(FetchStep::File(response.body)),
            Err(error) if !error.fatal && !error.unknown && !error.failure.retryable => {
                Ok(FetchStep::Unavailable(error.failure.message))
            }
            Err(error) => Err(error),
        }
    }

    fn split(message: Outgoing) -> Vec<Outgoing> {
        outgoing::split(message)
    }

    fn send(conversation: String, part: Outgoing) -> Result<HttpRequest, Fault> {
        Ok(outgoing::send(conversation, part))
    }

    fn answered(response: HttpResponse) -> Result<Option<String>, Fault> {
        if response.status == 204 {
            return Ok(None);
        }
        let body: serde_json::Value = wire::answer(response)?;
        match body {
            serde_json::Value::Array(_) => Ok(None),
            serde_json::Value::Object(message) => message
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| Some(id.into()))
                .ok_or_else(|| Fault {
                    unknown: true,
                    ..wire::invalid("message ID")
                }),
            _ => Err(Fault {
                unknown: true,
                ..wire::invalid("HTTP response shape")
            }),
        }
    }

    fn respond(handle: String, text: Option<String>) -> Result<Option<HttpRequest>, Fault> {
        incoming::respond(&handle, text)
    }

    #[expect(
        clippy::expect_used,
        reason = "commands consume the identity returned by identified"
    )]
    fn commands(identity: Identity, commands: Vec<Command>) -> Vec<HttpRequest> {
        let facts: Facts = serde_json::from_str(&identity.facts).expect("identified Discord facts");
        vec![wire::request(
            "PUT",
            format!("/applications/{}/commands", wire::path(&facts.application)),
            Some(json!(incoming::commands(commands))),
        )]
    }

    fn activity(
        conversation: String,
        _replies_here: bool,
        shown: Option<String>,
        update: ActivityUpdate,
    ) -> Option<HttpRequest> {
        outgoing::activity(conversation, shown, update)
    }

    fn presence(conversation: String, _view: ActivityView) -> Option<HttpRequest> {
        Some(wire::request(
            "POST",
            format!("/channels/{}/typing", wire::path(&conversation)),
            None,
        ))
    }

    fn open_conversation(beside: String, session: Vec<String>) -> Option<HttpRequest> {
        // The conversation ID cannot tell a direct message, thread or forum post from a channel
        // that takes threads; Discord refuses the first three, and `opened` gives that failure.
        Some(wire::request(
            "POST",
            format!("/channels/{}/threads", wire::path(&beside)),
            Some(json!({
                "name": enco_sdk::channel::child_title(&session, THREAD_NAME_CHARS), "type": 11,
            })),
        ))
    }

    fn opened(_beside: String, response: HttpResponse) -> Result<String, Fault> {
        #[derive(Deserialize)]
        struct Thread {
            id: String,
        }
        let thread: Thread = wire::answer(response)?;
        Ok(thread.id)
    }

    fn close_conversation(conversation: String) -> Option<HttpRequest> {
        Some(wire::request(
            "PATCH",
            format!("/channels/{}", wire::path(&conversation)),
            Some(json!({"archived": true, "locked": true})),
        ))
    }
}

#[cfg(test)]
mod tests;
