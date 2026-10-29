//! Message identity, addressing and segments decoded at the OneBot boundary.
use crate::{
    channel::{Fault, Inbound},
    content,
    wire::{self, Conversation, Target},
};
use enco_sdk::channel::{Action, CommandKind, typed_command};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Header {
    post_type: String,
}
#[derive(Deserialize)]
struct Message {
    message_type: String,
    message_id: i32,
    user_id: u64,
    group_id: Option<u64>,
    #[serde(default)]
    sender: Sender,
    message: Value,
}

/// Who sent a message, as far as OneBot reports it: it gives each field only when it can.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct Sender {
    nickname: Option<String>,
    card: Option<String>,
}

impl Sender {
    /// The name QQ shows: the group card, or the nickname without one.
    pub(super) fn name(self) -> Option<String> {
        [self.card, self.nickname]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
    }
}
pub(super) fn message(frame: &str, account: &str) -> Result<Option<Inbound>, Fault> {
    let value: Value = serde_json::from_str(frame).map_err(|_| wire::invalid("event JSON"))?;
    let header = Header::deserialize(&value).map_err(|_| wire::invalid("event post_type"))?;
    if header.post_type != "message" {
        return Ok(None);
    }
    let message: Message =
        serde_json::from_value(value).map_err(|_| wire::invalid("message event"))?;
    let sender = message.user_id.to_string();
    if sender == account {
        return Ok(None);
    }
    let direct = match message.message_type.as_str() {
        "private" => true,
        "group" => false,
        _ => return Err(wire::invalid("message_type")),
    };
    let conversation = if direct {
        Conversation::Private(message.user_id)
    } else {
        Conversation::Group(
            message
                .group_id
                .ok_or_else(|| wire::invalid("message group_id"))?,
        )
    };
    let Value::Array(segments) = &message.message else {
        return Err(wire::invalid(
            "message array; set SnowLuma messageFormat to array",
        ));
    };
    let decoded = content::decode(segments, Some(account));
    let mut content = decoded.content;
    let reply_to = match &decoded.quote {
        Some(Target::Reply(id)) => Some(id.to_string()),
        Some(Target::Forward(_)) | None => None,
    };
    content.quote = decoded.quote.map(Target::quote);
    if content.is_empty() {
        return Ok(None);
    }
    let command = if content.images.is_empty() && content.unviewable.is_empty() {
        command(&content.text)
    } else {
        None
    };
    let addressed = direct || decoded.mentioned || command.is_some();
    Ok(Some(Inbound {
        key: format!("{conversation}/{}", message.message_id),
        conversation: conversation.to_string(),
        message: message.message_id.to_string(),
        sender: Some(sender),
        sender_name: message.sender.name(),
        direct,
        addressed,
        reply_to,
        respond: None,
        action: command.unwrap_or(Action::Message(content)),
    }))
}

fn command(text: &str) -> Option<Action> {
    let (name, argument) = typed_command(text)?;
    CommandKind::from_name(name).and_then(|kind| kind.action(argument))
}
