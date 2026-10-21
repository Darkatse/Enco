//! Telegram-specific configuration, transport evidence and text limits.
mod attachments;
mod supervisor;
use crate::channel::{
    Action, Adapter, AttachmentContent, ChannelError, Fault, Identity, Input, Update,
};
use async_trait::async_trait;
use enco_core::Failure;
use enco_kernel::{SettingsError, TableOwner, parse_table};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{num::NonZeroU64, time::Duration};
pub use supervisor::TelegramChannel;

use crate::limits::{TELEGRAM_MESSAGE_CHARS, TELEGRAM_POLL_SECONDS, TELEGRAM_SEND_SECONDS};

pub struct Telegram {
    client: reqwest::Client,
    base: String,
    token: String,
    account: String,
}

impl Telegram {
    pub fn new(token: String, api_base: &str) -> Result<Self, ChannelError> {
        Self::with_base(token, &ApiBase::try_from(api_base.to_owned())?)
    }

    fn with_base(token: String, api_base: &ApiBase) -> Result<Self, ChannelError> {
        let (account, secret) = token
            .split_once(':')
            .ok_or_else(|| ChannelError::Config("invalid bot token".into()))?;
        if account.parse::<NonZeroU64>().is_err()
            || secret.is_empty()
            || !secret
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(ChannelError::Config("invalid bot token".into()));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ChannelError::Config(error.without_url().to_string()))?;
        Ok(Self {
            client,
            base: api_base.0.clone(),
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
#[serde(deny_unknown_fields)]
struct TelegramSettings {
    token_env: String,
    owner_user_id: NonZeroU64,
    #[serde(default)]
    api_base: ApiBase,
}

impl TelegramSettings {
    fn from_document(document: &Value) -> Result<Option<Self>, SettingsError> {
        let settings: Option<Self> = parse_table(document, "telegram")?;
        if let Some(settings) = &settings
            && settings.token_env.trim().is_empty()
        {
            return Err(SettingsError::new(
                "telegram",
                "token_env must not be empty",
            ));
        }
        Ok(settings)
    }

    fn adapter(&self) -> Result<Telegram, ChannelError> {
        let token = std::env::var(&self.token_env).map_err(|_| {
            ChannelError::Config(format!(
                "bot token environment variable {} is not set",
                self.token_env
            ))
        })?;
        Telegram::with_base(token, &self.api_base)
    }
}

/// Telegram owns its table and declares no model endpoints.
pub const TABLE_OWNER: TableOwner = TableOwner {
    tables: &[enco_kernel::ConfigTable::Single("telegram")],
    declare: |document, _kernel| {
        TelegramSettings::from_document(document)?;
        Ok(vec![])
    },
};

/// URL policy is established once when configuration enters the host.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
struct ApiBase(String);

impl Default for ApiBase {
    fn default() -> Self {
        Self("https://api.telegram.org".into())
    }
}

impl TryFrom<String> for ApiBase {
    type Error = ChannelError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = reqwest::Url::parse(&value)
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
        Ok(Self(url.as_str().trim_end_matches('/').into()))
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
    #[serde(flatten)]
    content: attachments::Content,
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
                    action: message.content.action(),
                });
                Ok(Update {
                    protocol: json!({"offset": offset}),
                    input,
                })
            })
            .collect()
    }

    async fn fetch(&self, locator: &str) -> Result<AttachmentContent, Fault> {
        match attachments::fetch(self, locator).await {
            Ok(bytes) => Ok(AttachmentContent::Image(bytes)),
            // Telegram refused this file for good; the connection itself is still usable.
            Err(error) if !error.fatal && !error.unknown && !error.failure.retryable => {
                Ok(AttachmentContent::Unavailable(error.failure.message))
            }
            Err(error) => Err(error),
        }
    }

    fn split(&self, text: &str) -> Vec<String> {
        split(text)
    }

    async fn send(&self, target: &str, text: &str) -> Result<(), Fault> {
        let value = self
            .request(
                "sendRichMessage",
                json!({"chat_id": target, "rich_message": {"markdown": text}}),
                TELEGRAM_SEND_SECONDS,
            )
            .await?;
        if value.get("message_id").and_then(Value::as_i64).is_none() {
            return Err(invalid_response("sendRichMessage"));
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
        _ => Action::Message(text.into()),
    }
}

fn split(mut text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    while !text.is_empty() {
        let mut end = text
            .char_indices()
            .nth(TELEGRAM_MESSAGE_CHARS)
            .map_or(text.len(), |(index, _)| index);
        if end < text.len() {
            end = text[..end].rfind('\n').map_or(end, |index| index + 1);
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
