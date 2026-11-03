//! Segments of incoming and quoted messages share one decoder; a malformed segment stays visible
//! as unviewable and the rest of the message stays.
use crate::{
    faces, files,
    wire::{Locator, Target},
};
use enco_sdk::{
    channel::{Attachment, AttachmentKind, Content},
    types::{Unviewable, UnviewableReason},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

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
struct Id {
    id: String,
}
#[derive(Deserialize)]
struct Media {
    #[serde(default)]
    url: String,
}
#[derive(Deserialize)]
struct File {
    file_id: String,
    file_hash: Option<String>,
}

/// A message's segments, decoded in order.
pub(super) struct Segments {
    /// The text and attachments, without a quote.
    pub(super) content: Content,
    /// Whether an at segment names the bot.
    pub(super) mentioned: bool,
    /// The message a reply segment answers, or the merged forward a forward segment carries.
    pub(super) quote: Option<Target>,
}

/// Decodes `segments`.
///
/// `bot` is the bot's account for an incoming message's own segments: only those leave out
/// mentions of it before any text and answer a message with a reply segment. A quoted message
/// shows every mention as written, and its own reply is not shown.
pub(super) fn decode(segments: &[Value], bot: Option<&str>, group_id: Option<u64>) -> Segments {
    let mut decoder = Decoder {
        bot,
        group_id,
        leading_mention: false,
        decoded: Segments {
            content: Content::default(),
            mentioned: false,
            quote: None,
        },
    };
    for value in segments {
        decoder.segment(value);
    }
    let mut decoded = decoder.decoded;
    if decoder.leading_mention {
        // QQ puts a space after an @; with the @ left out, it separates nothing.
        decoded.content.text = decoded.content.text.trim_start().to_owned();
    }
    decoded
}

/// Decodes one message's segments in order.
struct Decoder<'a> {
    /// The bot's account, for an incoming message's own segments.
    bot: Option<&'a str>,
    /// The group the message was sent in, absent in private chats or contextless forwards.
    group_id: Option<u64>,
    /// Whether a mention of the bot was left out before any text.
    leading_mention: bool,
    decoded: Segments,
}

impl Decoder<'_> {
    /// Adds the segment `value`; one that breaks OneBot's rules becomes unviewable.
    fn segment(&mut self, value: &Value) {
        let segment = match Segment::deserialize(value) {
            Ok(segment) => segment,
            Err(error) => {
                let invalid =
                    Unviewable::unavailable("message part", format!("invalid segment: {error}"));
                self.decoded.content.unviewable.push(invalid);
                return;
            }
        };
        if let Err(reason) = self.part(&segment) {
            let invalid =
                Unviewable::unavailable(description(&segment.kind, &segment.data), reason);
            self.decoded.content.unviewable.push(invalid);
        }
    }

    /// Adds `segment` to the decoded message, or says why it breaks OneBot's rules.
    fn part(&mut self, segment: &Segment) -> Result<(), String> {
        let content = &mut self.decoded.content;
        match segment.kind.as_str() {
            "text" => content
                .text
                .push_str(&data::<Text>(&segment.data, "text segment data.text")?.text),
            "at" => {
                let qq = data::<At>(&segment.data, "at segment data.qq")?.qq;
                let qq = if qq == "all" {
                    qq
                } else {
                    qq.parse::<u64>()
                        .map_err(|error| format!("invalid at segment data.qq: {error}"))?
                        .to_string()
                };
                if self.bot == Some(qq.as_str()) {
                    self.decoded.mentioned = true;
                    if content.text.trim().is_empty() {
                        self.leading_mention = true;
                        return Ok(());
                    }
                }
                content.text.push_str(&format!("@{qq}"));
            }
            "reply" if self.bot.is_none() => {}
            "reply" => {
                let id = data::<Id>(&segment.data, "reply segment data.id")?
                    .id
                    .parse()
                    .map_err(|error| format!("invalid reply segment data.id: {error}"))?;
                self.decoded.quote = Some(Target::Reply(id));
            }
            "forward" => {
                let id = data::<Id>(&segment.data, "forward segment data.id")?.id;
                self.decoded.quote = Some(Target::Forward(id));
            }
            kind @ ("image" | "record" | "video") => media(content, kind, &segment.data)?,
            "file" => file(content, &segment.data, self.group_id)?,
            "face" => {
                let id = data::<Id>(&segment.data, "face segment data.id")?
                    .id
                    .parse::<u32>()
                    .map_err(|error| format!("invalid face segment data.id: {error}"))?;
                let name = faces::NAMES
                    .iter()
                    .find_map(|(face, name)| (*face == id).then_some(*name));
                content.text.push_str(
                    &name.map_or_else(|| format!("[face {id}]"), |name| format!("[face: {name}]")),
                );
            }
            kind => content.unviewable.push(Unviewable {
                what: description(kind, &segment.data),
                reason: UnviewableReason::Unsupported,
            }),
        }
        Ok(())
    }
}

fn data<T: DeserializeOwned>(value: &Value, context: &str) -> Result<T, String> {
    T::deserialize(value).map_err(|error| format!("invalid {context}: {error}"))
}

/// What a segment of `kind` shows, in plain words.
fn description(kind: &str, data: &Value) -> String {
    match kind {
        "image" => AttachmentKind::Image.name(),
        "record" => AttachmentKind::VoiceMessage.name(),
        "video" => AttachmentKind::Video.name(),
        "mface" => AttachmentKind::Sticker.name(),
        "file" => data
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(AttachmentKind::File.name()),
        "json" | "xml" | "music" => "card",
        "contact" => "contact",
        "location" => "location",
        "poke" => "poke",
        "reply" => "replied message",
        "forward" => "forwarded messages",
        "face" => "face",
        "text" => "text",
        "at" => "mention",
        _ => "message part",
    }
    .into()
}

/// Media URLs are supplied by SnowLuma; its opaque media filenames do not describe the content.
fn media(content: &mut Content, kind: &str, value: &Value) -> Result<(), String> {
    let media = data::<Media>(value, &format!("{kind} segment data.url"))?;
    let name = description(kind, value);
    if media.url.is_empty() {
        content
            .unviewable
            .push(Unviewable::unavailable(name, files::NO_ADDRESS));
    } else {
        content.attachments.push(Attachment {
            name,
            locator: Locator::Download(media.url).to_string(),
        });
    }
    Ok(())
}

/// A file's locator holds only its IDs; SnowLuma gives the address, so content never chooses one.
fn file(content: &mut Content, value: &Value, group_id: Option<u64>) -> Result<(), String> {
    let file = data::<File>(value, "file segment data")?;
    if file.file_id.is_empty() {
        return Err("invalid file segment data.file_id: empty ID".into());
    }
    let name = description("file", value);
    let locator = if let Some(group_id) = group_id {
        Locator::GroupFile {
            group_id,
            file_id: file.file_id,
        }
    } else if let Some(file_hash) = file.file_hash.filter(|hash| !hash.is_empty()) {
        Locator::PrivateFile {
            file_id: file.file_id,
            file_hash,
        }
    } else {
        content.unviewable.push(Unviewable::unavailable(
            name,
            "QQ did not give what Enco needs to download this file",
        ));
        return Ok(());
    };
    content.attachments.push(Attachment {
        name,
        locator: locator.to_string(),
    });
    Ok(())
}
