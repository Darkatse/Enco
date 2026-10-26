//! Telegram Bot API translation, with transport, credentials and commits owned by the host.
mod incoming;
mod limits;
mod outgoing;
mod wire;

use bindings::exports::enco::plugin::{channel, lifecycle};
use channel::{Command, Fault, FetchStep, Identity, Outgoing, Request, Response, Update};
use enco_sdk::types::Failure;
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
                origin: None,
                placement: lifecycle::Placement::Path(vec!["/bot".into(), "/file/bot".into()]),
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

    fn poll(_identity: Identity, state: Option<String>) -> Result<Request, Fault> {
        let offset = incoming::offset(state.as_deref())?;
        Ok(request(
            "getUpdates",
            json!({"offset": offset, "timeout": POLL_SECONDS, "allowed_updates": ["message"]}),
            POLL_TIMEOUT_MS,
        ))
    }

    fn received(
        identity: Identity,
        state: Option<String>,
        response: Response,
    ) -> Result<Vec<Update>, Fault> {
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

    fn commands(commands: Vec<Command>) -> Vec<Request> {
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

    fn typing(conversation: String) -> Option<Request> {
        Some(request(
            "sendChatAction",
            json!({"chat_id": conversation, "action": "typing"}),
            SEND_TIMEOUT_MS,
        ))
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
            path: format!("/file/bot/{}", wire::path(&file.file_path)),
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
