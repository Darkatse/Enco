//! Telegram Bot API translation, with transport, credentials and commits owned by the host.
mod incoming;
mod limits;
mod outgoing;
mod wire;

use bindings::exports::enco::plugin::{channel, lifecycle};
use channel::{
    Arrival, Command, Fault, FetchStep, Identity, Outgoing, ReadStep, Reading, Request, Response,
};
use enco_sdk::{
    channel::{ActivityView, CANCEL_LABEL},
    types::Failure,
};
use limits::{POLL_SECONDS, POLL_TIMEOUT_MS, READ_TIMEOUT_MS, SEND_TIMEOUT_MS};
use serde::Deserialize;
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

#[derive(Deserialize)]
struct Account {
    id: i64,
    username: String,
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

    fn identify() -> Request {
        request("getMe", json!({}), READ_TIMEOUT_MS)
    }

    fn identified(response: Response) -> Result<Identity, Fault> {
        let account: Account = answer("getMe", response)?;
        Ok(Identity {
            protocol: "telegram".into(),
            account: account.id.to_string(),
            facts: json!({"username": account.username}).to_string(),
        })
    }

    fn read(_identity: Identity, state: Option<String>) -> Result<ReadStep, Fault> {
        let offset = incoming::offset(state.as_deref())?;
        Ok(ReadStep::Request(request(
            "getUpdates",
            json!({"offset": offset, "timeout": POLL_SECONDS, "allowed_updates": ["message", "callback_query"]}),
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

    fn fetch(locator: String, responses: Vec<Response>) -> Result<FetchStep, Fault> {
        match fetch_step(&locator, responses) {
            // Telegram rejected this file for good; the connection itself is still usable.
            Err(fault) if !fault.fatal && !fault.unknown && !fault.failure.retryable => {
                Ok(FetchStep::Unavailable(fault.failure.message))
            }
            result => result,
        }
    }

    fn split(message: Outgoing) -> Vec<Outgoing> {
        outgoing::split(message)
    }

    fn send(conversation: String, part: Outgoing) -> Result<Request, Fault> {
        Ok(outgoing::send(conversation, part))
    }

    fn answered(response: Response) -> Result<Option<String>, Fault> {
        match answer::<Answer>("Telegram request", response)? {
            Answer::Message(sent) => Ok(Some(sent.message_id.to_string())),
            Answer::Acknowledged(true) => Ok(None),
            Answer::Acknowledged(false) => Err(invalid_response("Telegram request")),
        }
    }

    fn commands(_identity: Identity, commands: Vec<Command>) -> Vec<Request> {
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
                            "command": incoming::command_name(command.kind),
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

    fn respond(handle: String, text: Option<String>) -> Result<Option<Request>, Fault> {
        Ok(Some(request(
            "answerCallbackQuery",
            json!({"callback_query_id": handle, "text": text}),
            SEND_TIMEOUT_MS,
        )))
    }

    fn activity(
        conversation: String,
        shown: Option<String>,
        activity: Option<ActivityView>,
    ) -> Option<Request> {
        match (shown, activity) {
            (Some(message), None) => Some(request(
                "deleteMessage",
                json!({"chat_id": conversation, "message_id": message}),
                SEND_TIMEOUT_MS,
            )),
            (shown, Some(activity)) => {
                let cancel = json!({"text": CANCEL_LABEL, "callback_data": "cancel"});
                let mut body = json!({
                    "chat_id": conversation,
                    "text": activity.text(),
                    "reply_markup": {"inline_keyboard": [[cancel]]},
                });
                let method = if let Some(message) = shown {
                    body["message_id"] = json!(message);
                    "editMessageText"
                } else {
                    "sendMessage"
                };
                Some(request(method, body, SEND_TIMEOUT_MS))
            }
            (None, None) => None,
        }
    }
}

fn fetch_step(locator: &str, responses: Vec<Response>) -> Result<FetchStep, Fault> {
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
        return Ok(FetchStep::Request(Request {
            method: "GET".into(),
            url: format!("/file/bot/{}", wire::path(&file.file_path)),
            headers: vec![],
            body: None,
            timeout_ms: Some(READ_TIMEOUT_MS),
        }));
    };
    if !(200..300).contains(&response.status) {
        return Err(wire::http_error("download file", response.status, None));
    }
    Ok(FetchStep::Image(response.body))
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
