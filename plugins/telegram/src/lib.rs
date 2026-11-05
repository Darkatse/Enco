//! Telegram Bot API translation, with transport, credentials and commits owned by the host.
mod activity;
mod content;
mod incoming;
mod limits;
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
use limits::{POLL_SECONDS, POLL_TIMEOUT_MS, READ_TIMEOUT_MS, SEND_TIMEOUT_MS};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wire::{answer, invalid_response, request};

enco_sdk::export!(channel, Plugin);
struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        enco_sdk::protocol::empty_config(&config)?;
        Ok(lifecycle::Description {
            summary: "Telegram Bot API messaging".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "token".into(),
                placements: vec![lifecycle::Placement {
                    origin: None,
                    location: lifecycle::Location::Path(vec!["/bot".into(), "/file/bot".into()]),
                }],
                required: true,
            }],
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

/// The bot's account, as `getMe` reports it.
#[derive(Deserialize)]
struct Account {
    id: i64,
    #[serde(flatten)]
    facts: Facts,
}

/// What the mapper needs to know about the bot later, kept as the identity's facts.
#[derive(Deserialize, Serialize)]
struct Facts {
    username: String,
}

impl Facts {
    /// The facts `identified` put in `identity`; a `plugin.contract` failure when the host passed
    /// back anything else.
    fn of(identity: &Identity) -> Result<Self, Fault> {
        serde_json::from_str(&identity.facts).map_err(|_| Fault {
            failure: Failure {
                code: "plugin.contract".into(),
                message: "identity facts are not the ones `identified` returns".into(),
                retryable: false,
            },
            unknown: false,
            fatal: true,
            retry_after: None,
        })
    }
}

#[derive(Deserialize)]
struct File {
    file_path: String,
}

#[derive(Deserialize)]
struct Sent {
    message_id: i64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Answer {
    Message(Sent),
    Acknowledged(bool),
}

impl channel::Guest for Plugin {
    fn origins() -> Vec<String> {
        vec![]
    }

    fn pacing() -> Option<channel::PacingGap> {
        None
    }

    fn identify() -> HttpRequest {
        request("getMe", json!({}), READ_TIMEOUT_MS)
    }

    fn identified(response: HttpResponse) -> Result<Identity, Fault> {
        let account: Account = answer("getMe", response)?;
        Ok(Identity {
            protocol: "telegram".into(),
            account: account.id.to_string(),
            facts: json!(account.facts).to_string(),
        })
    }

    fn read(_identity: Identity, state: Option<String>) -> Result<ReadStep, Fault> {
        let offset = incoming::offset(state.as_deref())?;
        Ok(ReadStep::Request(request(
            "getUpdates",
            json!({"offset": offset, "timeout": POLL_SECONDS, "allowed_updates": ["message", "callback_query", "stopped_message_generation"]}),
            POLL_TIMEOUT_MS,
        )))
    }

    fn received(
        identity: Identity,
        state: Option<String>,
        arrival: Arrival,
    ) -> Result<Reading, Fault> {
        let Arrival::Response(response) = arrival else {
            return Err(invalid_response("Telegram read arrival"));
        };
        incoming::received(identity, state, response)
    }

    fn fetch(
        locator: String,
        responses: Vec<HttpResponse>,
        _limit: u32,
    ) -> Result<FetchStep, Fault> {
        match fetch_step(&locator, responses) {
            // Telegram rejected this file for good; the connection itself is still usable.
            Err(fault) if !fault.fatal && !fault.unknown && !fault.failure.retryable => {
                Ok(FetchStep::Unavailable(format!(
                    "Telegram did not hand over this file: {}",
                    fault.failure.message
                )))
            }
            result => result,
        }
    }

    fn split(message: Outgoing) -> Vec<Outgoing> {
        outgoing::split(message)
    }

    fn send(conversation: String, part: Outgoing) -> Result<HttpRequest, Fault> {
        Ok(outgoing::send(conversation, part))
    }

    fn answered(response: HttpResponse) -> Result<Option<String>, Fault> {
        match answer::<Answer>("Telegram request", response)? {
            Answer::Message(sent) => Ok(Some(sent.message_id.to_string())),
            Answer::Acknowledged(true) => Ok(None),
            Answer::Acknowledged(false) => Err(invalid_response("Telegram request")),
        }
    }

    fn commands(_identity: Identity, commands: Vec<Command>) -> Vec<HttpRequest> {
        [("all_private_chats", true), ("all_group_chats", false)]
            .into_iter()
            .map(|(scope, direct)| {
                let commands: Vec<Value> = commands
                    .iter()
                    .filter(|command| {
                        if direct {
                            command.direct
                        } else {
                            command.group
                        }
                    })
                    .map(|command| {
                        json!({
                            "command": command.kind.name(),
                            "description": command.description,
                        })
                    })
                    .collect();
                request(
                    "setMyCommands",
                    json!({"scope": {"type": scope}, "commands": commands}),
                    SEND_TIMEOUT_MS,
                )
            })
            .collect()
    }

    fn respond(handle: String, text: Option<String>) -> Result<Option<HttpRequest>, Fault> {
        Ok(Some(request(
            "answerCallbackQuery",
            json!({"callback_query_id": handle, "text": text}),
            SEND_TIMEOUT_MS,
        )))
    }

    fn activity(
        conversation: String,
        replies_here: bool,
        shown: Option<String>,
        update: ActivityUpdate,
    ) -> Option<HttpRequest> {
        activity::activity(conversation, replies_here, shown, update)
    }

    fn presence(conversation: String, view: ActivityView) -> Option<HttpRequest> {
        activity::presence(conversation, view)
    }

    fn open_conversation(beside: String, session: Vec<String>) -> Option<HttpRequest> {
        activity::open(beside, session)
    }

    fn opened(beside: String, response: HttpResponse) -> Result<String, Fault> {
        activity::opened(beside, response)
    }

    fn close_conversation(conversation: String) -> Option<HttpRequest> {
        activity::close(conversation)
    }
}

fn fetch_step(locator: &str, responses: Vec<HttpResponse>) -> Result<FetchStep, Fault> {
    let mut responses = responses.into_iter();
    let Some(response) = responses.next() else {
        return Ok(FetchStep::Request(request(
            "getFile",
            json!({"file_id": locator}),
            READ_TIMEOUT_MS,
        )));
    };
    let file: File = answer("getFile", response)?;
    let Some(response) = responses.next() else {
        return Ok(FetchStep::Request(HttpRequest {
            method: "GET".into(),
            url: format!("/file/bot/{}", wire::path(&file.file_path)),
            headers: vec![],
            body: None,
            timeout_ms: Some(READ_TIMEOUT_MS),
        }));
    };
    if !(200..300).contains(&response.status) {
        return Err(wire::http_error(
            "download file",
            response.status,
            None,
            None,
        ));
    }
    Ok(FetchStep::File(response.body))
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
