//! Updates, addressing and attachments translated at the protocol boundary.
use crate::{
    channel::{
        Action, Attachment, CommandKind, Content, Fault, Identity, Inbound, Reading, Response,
    },
    wire::{answer, invalid_response},
};
use enco_sdk::types::{Failure, Unviewable, UnviewableReason};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
struct OffsetState {
    offset: i64,
}
#[derive(Deserialize)]
struct Facts {
    username: String,
}
#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<Message>,
    callback_query: Option<Callback>,
}
#[derive(Deserialize)]
struct Callback {
    id: String,
    from: User,
    message: Option<Message>,
    data: Option<String>,
}
#[derive(Deserialize)]
struct Message {
    message_id: i64,
    chat: Chat,
    from: Option<User>,
    reply_to_message: Option<Replied>,
    text: Option<String>,
    caption: Option<String>,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    caption_entities: Vec<Entity>,
    #[serde(default)]
    photo: Vec<Photo>,
    document: Option<Document>,
    #[serde(flatten)]
    other: Map<String, Value>,
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
    first_name: String,
    last_name: Option<String>,
}

impl User {
    /// The name Telegram shows for this user: the first name, then the last name when there is one.
    fn name(&self) -> String {
        match &self.last_name {
            Some(last) => format!("{} {last}", self.first_name),
            None => self.first_name.clone(),
        }
    }
}
#[derive(Deserialize)]
struct Replied {
    message_id: i64,
    from: Option<User>,
}
#[derive(Deserialize)]
struct Entity {
    #[serde(rename = "type")]
    kind: String,
    offset: usize,
    length: usize,
    user: Option<User>,
}
#[derive(Deserialize)]
struct Photo {
    file_id: String,
    width: u32,
    height: u32,
}
#[derive(Deserialize)]
struct Document {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

pub(super) fn offset(state: Option<&str>) -> Result<i64, Fault> {
    match state {
        None => Ok(0),
        Some(state) => serde_json::from_str::<OffsetState>(state)
            .map(|state| state.offset)
            .map_err(|_| Fault {
                failure: Failure {
                    code: "channel.invalid_state".into(),
                    message: "invalid Telegram offset state".into(),
                    retryable: false,
                },
                unknown: false,
                fatal: true,
                retry_after: None,
            }),
    }
}

pub(super) fn received(
    identity: Identity,
    state: Option<String>,
    response: Response,
) -> Result<Reading, Fault> {
    let mut offset = offset(state.as_deref())?;
    let facts: Facts = serde_json::from_str(&identity.facts).map_err(|_| Fault {
        failure: Failure {
            code: "plugin.contract".into(),
            message: "identity facts are not the ones `identified` returns".into(),
            retryable: false,
        },
        unknown: false,
        fatal: true,
        retry_after: None,
    })?;
    let updates: Vec<TelegramUpdate> = answer("getUpdates", response)?;
    let mut messages = Vec::new();
    for update in updates {
        let next = update
            .update_id
            .checked_add(1)
            .ok_or_else(|| invalid_response("getUpdates update_id"))?;
        if next <= offset {
            return Err(invalid_response("getUpdates update order"));
        }
        offset = next;
        let key = update.update_id.to_string();
        let message = if let Some(callback) = update.callback_query {
            callback_inbound(callback, key)?
        } else {
            update
                .message
                .map(|message| inbound(message, key, &identity.account, &facts.username))
                .transpose()?
                .flatten()
        };
        if let Some(message) = message {
            messages.push(message);
        }
    }
    Ok(Reading {
        messages,
        state: Some(json!({"offset": offset}).to_string()),
        frames: vec![],
        acknowledgments: vec![],
        timer: None,
        reconnect: false,
    })
}

fn callback_inbound(callback: Callback, key: String) -> Result<Option<Inbound>, Fault> {
    if callback.data.as_deref() != Some("cancel") {
        return Ok(None);
    }
    let message = callback
        .message
        .ok_or_else(|| invalid_response("callback_query message"))?;
    Ok(Some(Inbound {
        key,
        conversation: message.chat.id.to_string(),
        message: message.message_id.to_string(),
        sender: Some(callback.from.id.to_string()),
        sender_name: Some(callback.from.name()),
        direct: message.chat.kind == "private",
        addressed: true,
        reply_to: None,
        respond: Some(callback.id),
        action: Action::Cancel,
    }))
}

fn inbound(
    message: Message,
    key: String,
    account: &str,
    username: &str,
) -> Result<Option<Inbound>, Fault> {
    let direct = message.chat.kind == "private";
    // Telegram commands are text messages; a caption is never one.
    let command = message
        .text
        .as_deref()
        .and_then(|text| command(text, username));
    let (text, entities) = if let Some(text) = message.text {
        (text, message.entities)
    } else {
        (
            message.caption.unwrap_or_default(),
            message.caption_entities,
        )
    };
    let addressed = direct
        || match command {
            Some(CommandTarget::OtherBot) => false,
            Some(CommandTarget::ThisBot(_)) => true,
            None => {
                message
                    .reply_to_message
                    .as_ref()
                    .and_then(|reply| reply.from.as_ref())
                    .is_some_and(|user| user.id.to_string() == account)
                    || mentioned(&text, &entities, account, username)?
            }
        };
    let content = content(text, message.photo, message.document, message.other);
    if content.text.is_empty() && content.images.is_empty() && content.unviewable.is_empty() {
        return Ok(None);
    }
    let action = match command {
        Some(CommandTarget::ThisBot(Some(action)))
            if content.images.is_empty() && content.unviewable.is_empty() =>
        {
            action
        }
        _ => Action::Message(content),
    };
    Ok(Some(Inbound {
        key,
        respond: None,
        conversation: message.chat.id.to_string(),
        message: message.message_id.to_string(),
        sender: message.from.as_ref().map(|user| user.id.to_string()),
        sender_name: message.from.as_ref().map(User::name),
        direct,
        addressed,
        reply_to: message
            .reply_to_message
            .map(|reply| reply.message_id.to_string()),
        action,
    }))
}

/// The name Telegram shows and parses for each command, without its leading `/`.
pub(super) fn command_name(kind: CommandKind) -> &'static str {
    match kind {
        CommandKind::Session => "session",
        CommandKind::Cancel => "cancel",
        CommandKind::Backtrack => "backtrack",
        CommandKind::Everyone => "everyone",
    }
}

/// The bot a leading `/command` is for.
enum CommandTarget {
    /// Another bot, named after `@`.
    OtherBot,
    /// This bot, named after `@` or by one of its commands; the action when it knows the command
    /// and its argument.
    ThisBot(Option<Action>),
}

/// The bot `text` commands; absent when it starts with no command, or with one that names no bot
/// and that this bot does not understand.
fn command(text: &str, username: &str) -> Option<CommandTarget> {
    let text = text.trim();
    let (head, argument) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let (name, target) = head
        .split_once('@')
        .map_or((head, None), |(name, target)| (name, Some(target)));
    let name = name.strip_prefix('/')?;
    if target.is_some_and(|target| !target.eq_ignore_ascii_case(username)) {
        return Some(CommandTarget::OtherBot);
    }
    let action = [
        CommandKind::Session,
        CommandKind::Cancel,
        CommandKind::Backtrack,
        CommandKind::Everyone,
    ]
    .into_iter()
    .find(|&kind| command_name(kind) == name)
    .and_then(|kind| action(kind, argument.trim()));
    (action.is_some() || target.is_some()).then_some(CommandTarget::ThisBot(action))
}

fn action(kind: CommandKind, argument: &str) -> Option<Action> {
    match (kind, argument) {
        (CommandKind::Session, "") => Some(Action::Sessions),
        (CommandKind::Session, name) => Some(Action::SelectSession(name.into())),
        (CommandKind::Cancel, "") => Some(Action::Cancel),
        (CommandKind::Backtrack, "") => Some(Action::Backtrack),
        (CommandKind::Everyone, "") => Some(Action::Everyone(None)),
        (CommandKind::Everyone, "on") => Some(Action::Everyone(Some(true))),
        (CommandKind::Everyone, "off") => Some(Action::Everyone(Some(false))),
        _ => None,
    }
}

fn mentioned(
    text: &str,
    entities: &[Entity],
    account: &str,
    username: &str,
) -> Result<bool, Fault> {
    for entity in entities {
        if entity.kind == "text_mention" {
            let user = entity
                .user
                .as_ref()
                .ok_or_else(|| invalid_response("getUpdates text_mention"))?;
            if user.id.to_string() == account {
                return Ok(true);
            }
        } else if entity.kind == "mention" {
            let units: Vec<u16> = text
                .encode_utf16()
                .skip(entity.offset)
                .take(entity.length)
                .collect();
            if units.len() != entity.length {
                return Err(invalid_response("getUpdates entity range"));
            }
            let mention = String::from_utf16(&units)
                .map_err(|_| invalid_response("getUpdates entity range"))?;
            if mention
                .strip_prefix('@')
                .is_some_and(|name| name.eq_ignore_ascii_case(username))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn content(
    text: String,
    photos: Vec<Photo>,
    document: Option<Document>,
    other: Map<String, Value>,
) -> Content {
    let mut content = Content {
        text,
        images: vec![],
        unviewable: vec![],
    };
    if let Some(photo) = photos
        .into_iter()
        .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height))
    {
        content.images.push(Attachment {
            what: "photo".into(),
            locator: photo.file_id,
        });
    }
    if let Some(document) = document {
        let mime = document.mime_type.as_deref().unwrap_or("unknown MIME type");
        let name = document.file_name.as_deref().unwrap_or("unnamed");
        let what = format!("file {name} ({mime})");
        if mime.starts_with("image/") {
            content.images.push(Attachment {
                what,
                locator: document.file_id,
            });
        } else {
            content.unviewable.push(Unviewable {
                what,
                reason: UnviewableReason::Unsupported,
            });
        }
    }
    for kind in [
        "voice",
        "audio",
        "video",
        "video_note",
        "animation",
        "sticker",
        "contact",
        "location",
        "venue",
        "poll",
        "story",
        "paid_media",
    ] {
        if other.contains_key(kind) {
            content.unviewable.push(Unviewable {
                what: format!("{kind} message"),
                reason: UnviewableReason::Unsupported,
            });
        }
    }
    content
}
