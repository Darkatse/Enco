//! Message identity, addressing and segments decoded at the OneBot boundary.
use crate::{
    channel::{Fault, Inbound},
    wire::{self, Conversation},
};
use enco_sdk::{
    channel::{Action, Attachment, CommandKind, Content, typed_command},
    types::{Unviewable, UnviewableReason},
};
use serde::{Deserialize, de::DeserializeOwned};
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
    sender: Sender,
    message: Value,
}
#[derive(Deserialize)]
struct Sender {
    nickname: String,
    card: Option<String>,
}
#[derive(Deserialize)]
struct Segment {
    #[serde(rename = "type")]
    kind: String,
    data: Value,
}
#[derive(Deserialize)]
struct Text {
    text: String,
}
#[derive(Deserialize)]
struct At {
    qq: String,
}
#[derive(Deserialize)]
struct Reply {
    id: String,
}
#[derive(Deserialize)]
struct Image {
    url: String,
}
#[derive(Deserialize)]
struct Face {
    id: String,
}

pub(super) fn message(frame: &str, account: &str) -> Result<Option<Inbound>, Fault> {
    let value: Value = serde_json::from_str(frame).map_err(|_| wire::invalid("event JSON"))?;
    let header = Header::deserialize(&value).map_err(|_| wire::invalid("event post_type"))?;
    if header.post_type != "message" {
        return Ok(None);
    }
    let message: Message = decode(value, "message event")?;
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
    if message.message.is_string() {
        return Err(wire::invalid(
            "message array; set SnowLuma messageFormat to array",
        ));
    }
    let segments: Vec<Segment> = decode(message.message, "message segment array")?;
    let (content, mentioned, reply_to) = content(segments, account)?;
    if content.text.is_empty() && content.images.is_empty() && content.unviewable.is_empty() {
        return Ok(None);
    }
    let command = if content.images.is_empty() && content.unviewable.is_empty() {
        command(&content.text)
    } else {
        None
    };
    let addressed = direct || mentioned || command.is_some();
    let name = message
        .sender
        .card
        .filter(|card| !card.is_empty())
        .unwrap_or(message.sender.nickname);
    Ok(Some(Inbound {
        key: format!("{conversation}/{}", message.message_id),
        conversation: conversation.to_string(),
        message: message.message_id.to_string(),
        sender: Some(sender),
        sender_name: Some(name),
        direct,
        addressed,
        reply_to,
        respond: None,
        action: command.unwrap_or(Action::Message(content)),
    }))
}

fn decode<T: DeserializeOwned>(value: Value, context: &str) -> Result<T, Fault> {
    serde_json::from_value(value).map_err(|_| wire::invalid(context))
}

fn command(text: &str) -> Option<Action> {
    let (name, argument) = typed_command(text)?;
    CommandKind::from_name(name).and_then(|kind| kind.action(argument))
}

fn content(
    segments: Vec<Segment>,
    account: &str,
) -> Result<(Content, bool, Option<String>), Fault> {
    let mut content = Content {
        text: String::new(),
        images: vec![],
        unviewable: vec![],
    };
    let mut mentioned = false;
    let mut leading_mention = false;
    let mut reply_to = None;
    for segment in segments {
        match segment.kind.as_str() {
            "text" => content
                .text
                .push_str(&decode::<Text>(segment.data, "text segment data.text")?.text),
            "at" => {
                let qq = decode::<At>(segment.data, "at segment data.qq")?.qq;
                let qq = if qq == "all" {
                    qq
                } else {
                    qq.parse::<u64>()
                        .map_err(|_| wire::invalid("at segment data.qq"))?
                        .to_string()
                };
                if qq == account {
                    mentioned = true;
                    if content.text.trim().is_empty() {
                        leading_mention = true;
                        continue;
                    }
                }
                content.text.push_str(&format!("@{qq}"));
            }
            "reply" => {
                let id = decode::<Reply>(segment.data, "reply segment data.id")?.id;
                reply_to = Some(
                    id.parse::<i32>()
                        .map_err(|_| wire::invalid("reply segment data.id"))?
                        .to_string(),
                );
            }
            "image" => content.images.push(Attachment {
                what: "image".into(),
                locator: decode::<Image>(segment.data, "image segment data.url")?.url,
            }),
            "face" => {
                let id = decode::<Face>(segment.data, "face segment data.id")?.id;
                let id = id
                    .parse::<u32>()
                    .map_err(|_| wire::invalid("face segment data.id"))?;
                content.text.push_str(&format!("[face:{id}]"));
            }
            kind => content.unviewable.push(Unviewable {
                what: format!("OneBot {kind} message"),
                reason: UnviewableReason::Unsupported,
            }),
        }
    }
    if leading_mention {
        // QQ puts a space after an @; with the @ gone, it separates nothing.
        content.text = content.text.trim_start().to_owned();
    }
    Ok((content, mentioned, reply_to))
}
