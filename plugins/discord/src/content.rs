//! Discord text, attachments, replies and forwarded snapshots; a part that breaks Discord's rules
//! becomes unviewable and the rest of the message stays.
use crate::incoming::User;
use enco_sdk::channel::{
    Attachment, Content, Quote, Quoted, QuotedMessage, decode_part, invalid_part,
};
use enco_sdk::types::{Datetime, QuoteKind, Unviewable, UnviewableReason};
use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::{Map, Value};

const PROTOCOL: &str = "Discord";
/// Discord's `DEFAULT` message reference type, which replies use.
const REPLY_REFERENCE: u8 = 0;
/// Discord's `FORWARD` message reference type: the reference names the message it copies, not one
/// it answers.
const FORWARD_REFERENCE: u8 = 1;
/// The component type that shows text; a Components V2 message, such as this bot's, carries its
/// text in these and leaves `content` empty.
const TEXT_DISPLAY: u8 = 10;

/// A message Discord delivered, decoded for the Session.
pub(super) struct Decoded {
    /// What it shows; leading mentions of this bot are left out of the text.
    pub(super) content: Content,
    /// Whether it mentions this bot or replies to one of its messages.
    pub(super) addressed: bool,
    /// The message it replies to; a forward replies to none.
    pub(super) reply_to: Option<String>,
}

/// Decodes the content of a message `body` that reached the bot with `account`.
pub(super) fn decode(body: &Map<String, Value>, account: &str) -> Decoded {
    let Body { content, mentions } = Body::decode(body, Some(account));
    let mut decoded = Decoded {
        content,
        addressed: mentions.iter().any(|user| user.id == account),
        reply_to: None,
    };
    if let Some(reference) = body.get("message_reference") {
        decoded.reference(reference, body, account);
    }
    decoded
}

#[derive(Deserialize)]
struct Reference {
    // Discord omits the type for its default.
    #[serde(rename = "type", default)]
    kind: u8,
    message_id: Option<String>,
}

/// The message a reply answers, as Discord copies it into the reply.
#[derive(Deserialize)]
struct Replied {
    id: String,
    author: User,
    /// The messages it forwards, when it is a forward.
    #[serde(default)]
    message_snapshots: Vec<Value>,
    #[serde(flatten)]
    message: Shown,
}

/// A forwarded message as Discord copies it, without its author.
#[derive(Deserialize)]
struct Snapshot {
    message: Shown,
}

/// A message Discord shows inside another.
#[derive(Deserialize)]
struct Shown {
    #[serde(deserialize_with = "datetime")]
    timestamp: Datetime,
    #[serde(flatten)]
    body: Map<String, Value>,
}

impl Decoded {
    /// Adds what the message's `reference` names: the message it replies to, or the messages it
    /// forwards.
    fn reference(&mut self, reference: &Value, body: &Map<String, Value>, account: &str) {
        let reference = match Reference::deserialize(reference) {
            Ok(reference) => reference,
            // Without the reference's type, whether the message replies or forwards is unknown.
            Err(error) => {
                let invalid = invalid_part(PROTOCOL, "quoted message", error);
                self.content.unviewable.push(invalid);
                return;
            }
        };
        let (kind, messages) = match reference.kind {
            REPLY_REFERENCE => {
                self.reply_to = reference.message_id;
                let messages = self.replied(body.get("referenced_message"), account);
                (QuoteKind::Reply, messages)
            }
            FORWARD_REFERENCE => {
                let snapshots = body.get("message_snapshots").unwrap_or(&Value::Null);
                let messages = match Vec::<Value>::deserialize(snapshots) {
                    Ok(snapshots) => forwarded(&snapshots),
                    Err(error) => Quoted::Unavailable(format!(
                        "invalid {PROTOCOL} forwarded messages: {error}"
                    )),
                };
                (QuoteKind::Forward, messages)
            }
            // Other references, such as a pin notice's, quote nothing.
            _ => return,
        };
        self.content.quote = Some(Quote { kind, messages });
    }

    /// The messages a reply shows, from Discord's copy of the replied message.
    ///
    /// The copy is absent when Discord did not fetch it and null once the message was deleted; it
    /// names the replied message and tells whether it is this bot's.
    fn replied(&mut self, referenced: Option<&Value>, account: &str) -> Quoted {
        let replied = match referenced.map(Option::<Replied>::deserialize) {
            None => return Quoted::Unavailable("the referenced message was not provided".into()),
            Some(Ok(None)) => return Quoted::Unavailable("the message was deleted".into()),
            Some(Err(error)) => {
                return Quoted::Unavailable(format!("invalid {PROTOCOL} replied message: {error}"));
            }
            Some(Ok(Some(replied))) => replied,
        };
        self.addressed |= replied.author.id == account;
        self.reply_to = Some(replied.id);
        // A reply to a forward shows the forwarded messages.
        if replied.message_snapshots.is_empty() {
            Quoted::Messages(vec![replied.message.quoted(Some(replied.author))])
        } else {
            forwarded(&replied.message_snapshots)
        }
    }
}

/// The messages a forward copies, from its `message_snapshots`; a malformed snapshot stays as a
/// message that shows only why.
fn forwarded(snapshots: &[Value]) -> Quoted {
    if snapshots.is_empty() {
        return Quoted::Unavailable("the forward carried no message".into());
    }
    let messages = snapshots
        .iter()
        .map(|value| match Snapshot::deserialize(value) {
            Ok(snapshot) => snapshot.message.quoted(None),
            Err(error) => {
                QuotedMessage::invalid(0, invalid_part(PROTOCOL, "forwarded message", error))
            }
        });
    Quoted::Messages(messages.collect())
}

impl Shown {
    /// This message by `author`, when Discord names one; a forwarded snapshot has none.
    fn quoted(self, author: Option<User>) -> QuotedMessage {
        let mut message = QuotedMessage::new(0, Body::decode(&self.body, None).content);
        message.sender_name = author.as_ref().map(User::name);
        message.sender = author.map(|user| user.id);
        message.sent_at = Some(self.timestamp);
        message
    }
}

/// A Discord timestamp, ISO 8601 with an offset, as the instant it names.
fn datetime<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Datetime, D::Error> {
    let time = chrono::DateTime::parse_from_rfc3339(&String::deserialize(deserializer)?)
        .map_err(D::Error::custom)?;
    Ok(Datetime {
        seconds: u64::try_from(time.timestamp()).map_err(D::Error::custom)?,
        nanoseconds: time.timestamp_subsec_nanos(),
    })
}

#[derive(Deserialize)]
struct File {
    filename: String,
    url: String,
    content_type: Option<String>,
}

#[derive(Deserialize)]
struct Component {
    #[serde(rename = "type")]
    kind: u8,
    content: Option<String>,
    #[serde(default)]
    components: Vec<Component>,
}

#[derive(Deserialize)]
struct Embed {
    #[serde(rename = "type")]
    kind: String,
}

/// One message body's content, with the users it mentions, which name its `<@id>` mentions.
///
/// Mentions only name text that stays visible as written, so one that cannot be read is skipped.
struct Body {
    content: Content,
    mentions: Vec<User>,
}

impl Body {
    /// Decodes `body`; leading mentions of `bot` are left out of the text when it is given.
    fn decode(body: &Map<String, Value>, bot: Option<&str>) -> Self {
        let mut content = Content::default();
        let mentions: Vec<User> = body
            .get("mentions")
            .and_then(|value| Vec::<Value>::deserialize(value).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|value| User::deserialize(value).ok())
            .collect();
        let text = text(body, &mut content.unviewable);
        content.text = humanize(&text, &mentions, bot);
        attachments(body, &mut content);
        Self { content, mentions }
    }
}

/// The list in `body`'s `field`, empty when it is absent; when it is not a list, `unviewable`
/// gains `what`.
fn list(
    body: &Map<String, Value>,
    field: &str,
    what: &str,
    unviewable: &mut Vec<Unviewable>,
) -> Vec<Value> {
    body.get(field)
        .and_then(|value| decode_part(PROTOCOL, what, value, unviewable))
        .unwrap_or_default()
}

/// What the sender wrote: `content`, or the text displays of a Components V2 message, one per
/// line.
fn text(body: &Map<String, Value>, unviewable: &mut Vec<Unviewable>) -> String {
    let mut lines = vec![];
    if let Some(value) = body.get("content") {
        lines.extend(decode_part::<String>(PROTOCOL, "text", value, unviewable));
    }
    if let Some(value) = body.get("components")
        && let Some(components) = decode_part(PROTOCOL, "text", value, unviewable)
    {
        displays(components, &mut lines);
    }
    lines.retain(|line| !line.is_empty());
    lines.join("\n")
}

/// Adds the text of each text display in `components`, nested ones included, in order.
fn displays(components: Vec<Component>, lines: &mut Vec<String>) {
    for component in components {
        if component.kind == TEXT_DISPLAY {
            lines.extend(component.content);
        }
        displays(component.components, lines);
    }
}

/// Adds `body`'s files, stickers, embeds the sender wrote and poll to `content`.
fn attachments(body: &Map<String, Value>, content: &mut Content) {
    for value in list(body, "attachments", "attachments", &mut content.unviewable) {
        let Some(file) = decode_part::<File>(PROTOCOL, "file", &value, &mut content.unviewable)
        else {
            continue;
        };
        let what = format!("file {}", file.filename);
        if file
            .content_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            content.images.push(Attachment {
                what,
                locator: file.url,
            });
        } else {
            content.unviewable.push(Unviewable {
                what,
                reason: UnviewableReason::Unsupported,
            });
        }
    }
    let stickers = list(body, "sticker_items", "sticker", &mut content.unviewable).len();
    // Link previews and GIFs repeat a URL already in the text; only a rich embed is the sender's.
    let cards = list(body, "embeds", "card", &mut content.unviewable)
        .iter()
        .filter_map(|value| decode_part::<Embed>(PROTOCOL, "card", value, &mut content.unviewable))
        .filter(|embed| embed.kind == "rich")
        .count();
    let polls = usize::from(body.contains_key("poll"));
    for (what, count) in [("sticker", stickers), ("card", cards), ("poll", polls)] {
        for _ in 0..count {
            content.unviewable.push(Unviewable {
                what: what.into(),
                reason: UnviewableReason::Unsupported,
            });
        }
    }
}

/// `text` as people in the chat see it, with leading mentions of `bot` left out.
///
/// User mentions are `@name` from `mentions` and custom emoji are `:name:`; channel and role
/// mentions carry no name in the message and stay as written.
fn humanize(text: &str, mentions: &[User], bot: Option<&str>) -> String {
    let mut remaining = text;
    let mut result = String::new();
    while let Some(start) = remaining.find('<') {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let Some(end) = remaining.find('>') else {
            break;
        };
        let token = &remaining[1..end];
        let user = token.strip_prefix('@').and_then(|id| {
            let id = id.strip_prefix('!').unwrap_or(id);
            mentions.iter().find(|user| user.id == id)
        });
        if let Some(user) = user {
            if bot == Some(user.id.as_str()) && result.trim().is_empty() {
                result.clear();
                remaining = remaining[end + 1..].trim_start();
                continue;
            }
            result.push('@');
            result.push_str(&user.name());
        } else if let Some((name, _)) = token
            .strip_prefix(':')
            .or_else(|| token.strip_prefix("a:"))
            .and_then(|emoji| emoji.split_once(':'))
            .filter(|(name, id)| {
                !name.is_empty() && !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            result.push(':');
            result.push_str(name);
            result.push(':');
        } else {
            // A `<` that opens no mention or emoji is text; scanning resumes right after it.
            result.push('<');
            remaining = &remaining[1..];
            continue;
        }
        remaining = &remaining[end + 1..];
    }
    result.push_str(remaining);
    result
}
