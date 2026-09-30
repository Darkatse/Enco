//! Telegram-specific syntax, transport evidence and text limits.
use crate::channel::{Action, Adapter, ChannelError, Fault, Identity, Input, Update};
use async_trait::async_trait;
use enco_core::Failure;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

use crate::limits::{TELEGRAM_MESSAGE_UNITS, TELEGRAM_POLL_SECONDS, TELEGRAM_SEND_SECONDS};

pub struct Telegram {
    client: reqwest::Client,
    base: String,
    token: String,
    account: String,
}

impl Telegram {
    pub fn new(token: String, api_base: &str) -> Result<Self, ChannelError> {
        let (account, secret) = token
            .split_once(':')
            .ok_or_else(|| ChannelError::Config("invalid bot token".into()))?;
        if account.parse::<u64>().ok().filter(|id| *id > 0).is_none()
            || secret.is_empty()
            || !secret
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(ChannelError::Config("invalid bot token".into()));
        }
        let url = reqwest::Url::parse(api_base)
            .map_err(|_| ChannelError::Config("invalid channel api_base URL".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ChannelError::Config(
                "api_base must be an HTTP(S) URL without credentials, query or fragment".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ChannelError::Config(error.without_url().to_string()))?;
        Ok(Self {
            client,
            base: api_base.trim_end_matches('/').into(),
            account: account.into(),
            token,
        })
    }

    async fn request(&self, method: &str, body: Value, timeout: u64) -> Result<Value, Fault> {
        let response = self
            .client
            .post(format!("{}/bot{}/{}", self.base, self.token, method))
            .json(&body)
            .timeout(Duration::from_secs(timeout))
            .send()
            .await
            .map_err(|error| network(method, error))?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|error| {
            if status.is_success() {
                network(method, error)
            } else {
                http_error(method, status.as_u16(), None)
            }
        })?;
        // Preserve HTTP evidence even when the response body is malformed.
        let envelope: Result<Envelope, _> = serde_json::from_slice(&bytes);
        let envelope = match envelope {
            Ok(envelope) => envelope,
            Err(_) if !status.is_success() => {
                return Err(http_error(method, status.as_u16(), None));
            }
            Err(_) => return Err(invalid_response(method)),
        };
        if !envelope.ok || !status.is_success() {
            let code = if status.is_success() {
                envelope.error_code.unwrap_or(status.as_u16())
            } else {
                status.as_u16()
            };
            return Err(http_error(
                method,
                code,
                envelope.parameters.and_then(|p| p.retry_after),
            ));
        }
        envelope.result.ok_or_else(|| invalid_response(method))
    }
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    result: Option<Value>,
    error_code: Option<u16>,
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u64>,
}

#[derive(Deserialize)]
struct Protocol {
    offset: i64,
}

#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Deserialize)]
struct TelegramMessage {
    chat: Chat,
    from: Option<User>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct User {
    id: i64,
}

#[async_trait]
impl Adapter for Telegram {
    fn identity(&self) -> Identity {
        Identity {
            channel: "telegram".into(),
            account: self.account.clone(),
        }
    }
    fn initial_state(&self) -> Value {
        json!({"offset": 0})
    }

    async fn poll(&self, protocol: Value) -> Result<Vec<Update>, Fault> {
        let protocol: Protocol = serde_json::from_value(protocol).map_err(|_| invalid_state())?;
        let value = self.request("getUpdates", json!({"offset": protocol.offset, "timeout": TELEGRAM_POLL_SECONDS, "allowed_updates": ["message"]}), TELEGRAM_POLL_SECONDS + 10).await?;
        let updates: Vec<TelegramUpdate> =
            serde_json::from_value(value).map_err(|_| invalid_response("getUpdates"))?;
        updates
            .into_iter()
            .map(|update| {
                let offset = update
                    .update_id
                    .checked_add(1)
                    .ok_or_else(|| invalid_response("getUpdates"))?;
                if offset <= protocol.offset {
                    return Err(invalid_response("getUpdates"));
                }
                let input = update.message.map(|message| Input {
                    conversation: message.chat.id.to_string(),
                    sender: message.from.map(|sender| sender.id.to_string()),
                    direct: message.chat.kind == "private",
                    conversation_type: message.chat.kind,
                    action: message.text.map(parse_action),
                });
                Ok(Update {
                    protocol: json!({"offset": offset}),
                    input,
                })
            })
            .collect()
    }

    fn split(&self, text: &str) -> Vec<String> {
        split(text)
    }

    async fn send(&self, target: &str, text: &str) -> Result<(), Fault> {
        let value = self
            .request(
                "sendMessage",
                json!({"chat_id": target, "text": text}),
                TELEGRAM_SEND_SECONDS,
            )
            .await?;
        if value.get("message_id").and_then(Value::as_i64).is_none() {
            return Err(invalid_response("sendMessage"));
        }
        Ok(())
    }
}

fn parse_action(text: String) -> Action {
    let (command, argument) = text
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((text.trim(), ""));
    let command = command.split('@').next().unwrap_or(command);
    match (command, argument.trim()) {
        ("/session", "") => Action::Sessions,
        ("/session", name) => Action::SelectSession(name.into()),
        ("/cancel", "") => Action::Cancel,
        _ => Action::Message(text),
    }
}

fn split(mut text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    while !text.is_empty() {
        let mut units = 0;
        let mut end = 0;
        let mut newline = None;
        for (start, c) in text.char_indices() {
            units += c.len_utf16();
            if units > TELEGRAM_MESSAGE_UNITS {
                break;
            }
            end = start + c.len_utf8();
            if c == '\n' {
                newline = Some(end);
            }
        }
        if end < text.len() {
            end = newline.unwrap_or(end);
        }
        parts.push(text[..end].into());
        text = &text[end..];
    }
    parts
}

fn network(method: &str, error: reqwest::Error) -> Fault {
    let unsent = error.is_connect() || error.is_builder();
    let retryable = !error.is_builder();
    Fault {
        failure: Failure {
            code: "channel.network".into(),
            message: format!("{method}: {}", error.without_url()),
            retryable,
        },
        unknown: !unsent,
        fatal: false,
        retry_after: None,
    }
}

fn http_error(method: &str, status: u16, retry_after: Option<u64>) -> Fault {
    Fault {
        failure: Failure {
            code: format!("channel.http.{status}"),
            message: format!("{method} returned HTTP {status}"),
            retryable: status == 429 || status >= 500,
        },
        // Only a client rejection proves non-delivery. Reads may retry server errors, sends may not.
        unknown: !(400..500).contains(&status),
        fatal: matches!(status, 401 | 404),
        retry_after: retry_after.map(Duration::from_secs),
    }
}

fn invalid_response(method: &str) -> Fault {
    Fault {
        failure: Failure {
            code: "channel.bad_response".into(),
            message: format!("{method} returned an invalid response"),
            retryable: false,
        },
        unknown: true,
        fatal: false,
        retry_after: None,
    }
}

fn invalid_state() -> Fault {
    Fault {
        failure: Failure {
            code: "channel.invalid_state".into(),
            message: "invalid Telegram offset state".into(),
            retryable: false,
        },
        unknown: false,
        fatal: true,
        retry_after: None,
    }
}
