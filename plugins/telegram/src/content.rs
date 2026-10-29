//! Telegram text, attachments, replies and forwards; a part that breaks Telegram's rules becomes
//! unviewable and the rest of the message stays.
use crate::incoming::User;
use enco_sdk::channel::{
    Attachment, Content, Quote, Quoted, QuotedMessage, decode_part, invalid_part,
};
use enco_sdk::types::{Datetime, QuoteKind, Unviewable, UnviewableReason};
use serde::Deserialize;
use serde_json::{Map, Value};

const PROTOCOL: &str = "Telegram";

/// A message Telegram delivered, decoded for the Session.
pub(super) struct Decoded {
    /// What the sender wrote, without leading mentions of this bot, and the quote. A forward's text
    /// and attachments are the forwarded message's, so its own are empty.
    pub(super) content: Content,
    /// Whether it mentions this bot or replies to one of its messages; a forward does neither.
    pub(super) addressed: bool,
    /// The message it replies to.
    pub(super) reply_to: Option<String>,
}

/// Decodes the content of a message `body` that reached the bot with `account` and `username`.
pub(super) fn decode(body: &Map<String, Value>, account: &str, username: &str) -> Decoded {
    let mut content = content(body);
    if let Some(origin) = body.get("forward_origin") {
        // A forward passes on another sender's words, so they neither address nor command the bot.
        let messages = Quoted::Messages(vec![forwarded(content, origin)]);
        let quote = Quote {
            kind: QuoteKind::Forward,
            messages,
        };
        return Decoded {
            content: Content {
                quote: Some(quote),
                ..Content::default()
            },
            addressed: false,
            reply_to: None,
        };
    }
    let mut addressed = mentions(&mut content, body, account, username);
    let mut reply_to = None;
    if let Some(value) = body.get("reply_to_message") {
        let messages = match Replied::deserialize(value) {
            Ok(replied) => {
                addressed |= replied
                    .from
                    .as_ref()
                    .is_some_and(|user| user.id.to_string() == account);
                reply_to = Some(replied.message_id.to_string());
                Quoted::Messages(vec![replied.quoted()])
            }
            Err(error) => {
                Quoted::Unavailable(format!("invalid {PROTOCOL} replied message: {error}"))
            }
        };
        content.quote = Some(Quote {
            kind: QuoteKind::Reply,
            messages,
        });
    }
    Decoded {
        content,
        addressed,
        reply_to,
    }
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

/// A message `body`'s text, as the sender wrote it, and attachments.
fn content(body: &Map<String, Value>) -> Content {
    let mut content = Content::default();
    if let Some(value) = body.get("text").or_else(|| body.get("caption")) {
        content.text =
            decode_part(PROTOCOL, "text", value, &mut content.unviewable).unwrap_or_default();
    }
    if let Some(value) = body.get("photo") {
        match largest(value) {
            Ok(photo) => content.images.push(Attachment {
                what: "photo".into(),
                locator: photo.file_id,
            }),
            Err(error) => content
                .unviewable
                .push(invalid_part(PROTOCOL, "photo", error)),
        }
    }
    if let Some(value) = body.get("document")
        && let Some(document) =
            decode_part::<Document>(PROTOCOL, "file", value, &mut content.unviewable)
    {
        let what = format!(
            "file {}",
            document.file_name.as_deref().unwrap_or("unnamed")
        );
        if document
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
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
    for (field, what) in [
        ("voice", "voice message"),
        ("audio", "audio message"),
        ("video", "video"),
        ("video_note", "video message"),
        ("animation", "animation"),
        ("sticker", "sticker"),
        ("contact", "contact"),
        ("location", "location"),
        ("venue", "venue"),
        ("poll", "poll"),
        ("story", "story"),
        ("paid_media", "paid media"),
    ] {
        if body.contains_key(field) {
            content.unviewable.push(Unviewable {
                what: what.into(),
                reason: UnviewableReason::Unsupported,
            });
        }
    }
    content
}

/// The largest valid size of a `photo`, which Telegram sends as the sizes of one picture.
fn largest(photo: &Value) -> Result<Photo, String> {
    let sizes = Vec::<Value>::deserialize(photo).map_err(|error| error.to_string())?;
    let mut invalid = String::from("it has no sizes");
    let largest = sizes
        .iter()
        .filter_map(|size| {
            Photo::deserialize(size)
                .map_err(|error| invalid = error.to_string())
                .ok()
        })
        .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height));
    largest.ok_or(invalid)
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Entity {
    #[serde(rename = "mention")]
    Mention { offset: usize, length: usize },
    #[serde(rename = "text_mention")]
    TextMention {
        offset: usize,
        length: usize,
        user: User,
    },
    /// Formatting, links and the like, which do not change who the text mentions.
    #[serde(other)]
    Other,
}

/// Whether `body`'s text entities mention this bot; its mentions before any other text only say
/// whom the message is for, so they are left out of `content.text`.
///
/// Entities only annotate text that stays visible as written, so one that cannot be read is
/// skipped.
fn mentions(
    content: &mut Content,
    body: &Map<String, Value>,
    account: &str,
    username: &str,
) -> bool {
    let Some(Ok(entities)) = body
        .get("entities")
        .or_else(|| body.get("caption_entities"))
        .map(Vec::<Value>::deserialize)
    else {
        return false;
    };
    let units: Vec<u16> = content.text.encode_utf16().collect();
    let blank = |units: &[u16]| {
        units
            .iter()
            .all(|unit| char::from_u32(u32::from(*unit)).is_some_and(char::is_whitespace))
    };
    let mut addressed = false;
    let mut leading = 0;
    for value in &entities {
        let (offset, length, user) = match Entity::deserialize(value) {
            Ok(Entity::Mention { offset, length }) => (offset, length, None),
            Ok(Entity::TextMention {
                offset,
                length,
                user,
            }) => (offset, length, Some(user)),
            Ok(Entity::Other) | Err(_) => continue,
        };
        let Some(mention) = offset
            .checked_add(length)
            .and_then(|end| units.get(offset..end))
        else {
            continue;
        };
        let ours = match user {
            Some(user) => user.id.to_string() == account,
            None => String::from_utf16_lossy(mention)
                .strip_prefix('@')
                .is_some_and(|name| name.eq_ignore_ascii_case(username)),
        };
        addressed |= ours;
        if ours && units.get(leading..offset).is_some_and(blank) {
            leading = offset + length;
        }
    }
    if leading > 0 {
        content.text = String::from_utf16_lossy(&units[leading..])
            .trim_start()
            .into();
    }
    addressed
}

/// A chat a message was sent as, such as a group's anonymous admin or a channel posting in its
/// discussion group.
#[derive(Deserialize)]
struct SenderChat {
    id: i64,
    title: String,
}

/// The message a reply answers, as Telegram copies it into the reply.
#[derive(Deserialize)]
struct Replied {
    message_id: i64,
    date: u64,
    from: Option<User>,
    sender_chat: Option<SenderChat>,
    #[serde(flatten)]
    body: Map<String, Value>,
}

impl Replied {
    /// The replied message, by the identity it was sent as; a forward is by its original sender.
    fn quoted(self) -> QuotedMessage {
        let content = content(&self.body);
        if let Some(origin) = self.body.get("forward_origin") {
            return forwarded(content, origin);
        }
        let mut message = QuotedMessage::new(0, content);
        (message.sender, message.sender_name) = match (self.sender_chat, self.from) {
            (Some(chat), _) => (Some(chat.id.to_string()), Some(chat.title)),
            (None, Some(user)) => (Some(user.id.to_string()), Some(user.name())),
            (None, None) => (None, None),
        };
        message.sent_at = Some(Datetime {
            seconds: self.date,
            nanoseconds: 0,
        });
        message
    }
}

#[derive(Deserialize)]
struct NamedChat {
    title: String,
}
#[derive(Deserialize)]
#[serde(tag = "type")]
enum Origin {
    #[serde(rename = "user")]
    User { date: u64, sender_user: User },
    #[serde(rename = "hidden_user")]
    HiddenUser { date: u64, sender_user_name: String },
    #[serde(rename = "chat")]
    Chat {
        date: u64,
        sender_chat: NamedChat,
        author_signature: Option<String>,
    },
    #[serde(rename = "channel")]
    Channel {
        date: u64,
        chat: NamedChat,
        author_signature: Option<String>,
    },
}

/// Forwarded `content`, by the original sender its `origin` names; a malformed origin leaves the
/// sender unknown.
fn forwarded(content: Content, origin: &Value) -> QuotedMessage {
    let mut message = QuotedMessage::new(0, content);
    let Ok(origin) = Origin::deserialize(origin) else {
        return message;
    };
    let date = match origin {
        Origin::User { date, sender_user } => {
            message.sender = Some(sender_user.id.to_string());
            message.sender_name = Some(sender_user.name());
            date
        }
        Origin::HiddenUser {
            date,
            sender_user_name,
        } => {
            message.sender_name = Some(sender_user_name);
            date
        }
        Origin::Chat {
            date,
            sender_chat: chat,
            author_signature,
        }
        | Origin::Channel {
            date,
            chat,
            author_signature,
        } => {
            message.sender_name = author_signature;
            message.conversation_name = Some(chat.title);
            date
        }
    };
    message.sent_at = Some(Datetime {
        seconds: date,
        nanoseconds: 0,
    });
    message
}
