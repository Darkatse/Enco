//! What every channel mapper shares: commands, decoded content, the activity card's wording and
//! `multipart/form-data` uploads.
//!
//! Each protocol marks commands and lays the card out its own way; the names, meanings and words
//! are the same everywhere. [`export!`](crate::export) gives a channel plugin these same types, so
//! name them from here.
pub use crate::bindings::exports::enco::plugin::channel::{
    Action, ActivityView, Attachment, CommandKind, Content, Quote, Quoted, QuotedMessage,
    ToolActivity, ToolState,
};
use crate::types::{Unviewable, UnviewableReason};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt::Display;

impl Default for Content {
    /// No text, attachments or quote.
    fn default() -> Self {
        Self {
            text: String::new(),
            images: vec![],
            unviewable: vec![],
            quote: None,
        }
    }
}

impl Content {
    /// Whether it shows nothing: no text, attachments or quote.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.images.is_empty()
            && self.unviewable.is_empty()
            && self.quote.is_none()
    }
}

impl QuotedMessage {
    /// `content` shown `depth` forwards deep, with no sender, conversation or time yet.
    ///
    /// A quote inside `content` is left out: a quoted message's own quotes are not shown.
    pub fn new(depth: u8, content: Content) -> Self {
        Self {
            depth,
            sender: None,
            sender_name: None,
            conversation_name: None,
            sent_at: None,
            text: content.text,
            images: content.images,
            unviewable: content.unviewable,
        }
    }

    /// A message `depth` forwards deep that could not be decoded, showing only `why`.
    pub fn invalid(depth: u8, why: Unviewable) -> Self {
        Self::new(
            depth,
            Content {
                unviewable: vec![why],
                ..Content::default()
            },
        )
    }
}

impl Unviewable {
    /// `what`, which could not be retrieved for `reason`.
    pub fn unavailable(what: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            reason: UnviewableReason::Unavailable(reason.into()),
        }
    }
}

/// A part of a message that breaks `protocol`'s rules, such as `Discord`, kept as `what` and
/// unavailable because of `error`; the rest of the message stays.
pub fn invalid_part(protocol: &str, what: &str, error: impl Display) -> Unviewable {
    Unviewable::unavailable(what, format!("invalid {protocol} {what}: {error}"))
}

/// `value` decoded as a `T`; absent when it is not one, and `unviewable` gains `what` as an
/// [`invalid_part`] of `protocol`.
pub fn decode_part<T: DeserializeOwned>(
    protocol: &str,
    what: &str,
    value: &Value,
    unviewable: &mut Vec<Unviewable>,
) -> Option<T> {
    T::deserialize(value)
        .map_err(|error| unviewable.push(invalid_part(protocol, what, error)))
        .ok()
}

impl CommandKind {
    /// The shared name, without a leading `/` or a protocol's decoration.
    pub fn name(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Cancel => "cancel",
            Self::Backtrack => "backtrack",
            Self::Everyone => "everyone",
        }
    }

    /// The command with this exact, case-sensitive name; unknown names are ordinary text.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Session, Self::Cancel, Self::Backtrack, Self::Everyone]
            .into_iter()
            .find(|kind| kind.name() == name)
    }

    /// What the command asks for with `argument`, compared without surrounding whitespace;
    /// unknown arguments are ordinary text.
    pub fn action(self, argument: &str) -> Option<Action> {
        match (self, argument.trim()) {
            (Self::Session, "") => Some(Action::Sessions),
            (Self::Session, name) => Some(Action::SelectSession(name.into())),
            (Self::Cancel, "") => Some(Action::Cancel),
            (Self::Backtrack, "") => Some(Action::Backtrack),
            (Self::Everyone, "") => Some(Action::Everyone(None)),
            (Self::Everyone, "on") => Some(Action::Everyone(Some(true))),
            (Self::Everyone, "off") => Some(Action::Everyone(Some(false))),
            _ => None,
        }
    }
}

/// The command typed at the start of `text`, as its name and the rest as its argument; absent
/// when `text` starts otherwise.
///
/// The name follows a half-width `/` up to the first whitespace and keeps any decoration the
/// protocol adds, such as Telegram's `@bot`.
pub fn typed_command(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    let (head, argument) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    Some((head.strip_prefix('/')?, argument))
}

/// The label of the activity card's button that cancels the Run.
pub const CANCEL_LABEL: &str = "Cancel";

impl ActivityView {
    /// The activity card's text: `Working` and the elapsed `m:ss`, then a line per tool.
    ///
    /// Each tool line is a mark, the tool's name and, after ` · `, its argument summary when it
    /// has one; the marks are `…` running, `✓` ok, `✗` failed and `?` unknown.
    pub fn text(&self) -> String {
        let mut text = format!(
            "Working {}:{:02}",
            self.elapsed_seconds / 60,
            self.elapsed_seconds % 60
        );
        for tool in &self.tools {
            let mark = match tool.state {
                ToolState::Running => "…",
                ToolState::Ok => "✓",
                ToolState::Failed => "✗",
                ToolState::Unknown => "?",
            };
            text.push_str(&format!("\n{mark} {}", tool.name));
            if !tool.summary.is_empty() {
                text.push_str(&format!(" · {}", tool.summary));
            }
        }
        text
    }
}

/// One part of a `multipart/form-data` body.
pub struct FormPart<'a> {
    /// The form field's name.
    pub name: &'a str,
    /// The uploaded file's name, sent with `\r`, `\n` and `"` percent-encoded; absent for a field
    /// that is not a file.
    pub filename: Option<&'a str>,
    /// The part's `Content-Type`; absent leaves the receiver's default, `text/plain`.
    pub content_type: Option<&'a str>,
    /// The part's bytes, sent as they are.
    pub data: &'a [u8],
}

/// Encode `parts` in order as `multipart/form-data`, returning the `content-type` header value and
/// the body.
///
/// The boundary occurs in no part's name, file name or bytes.
pub fn multipart(parts: &[FormPart<'_>]) -> (String, Vec<u8>) {
    let mut boundary = String::from("enco-boundary");
    while parts.iter().any(|part| {
        [
            part.name.as_bytes(),
            part.filename.unwrap_or_default().as_bytes(),
            part.data,
        ]
        .into_iter()
        .any(|bytes| {
            bytes
                .windows(boundary.len())
                .any(|window| window == boundary.as_bytes())
        })
    }) {
        boundary.push('_');
    }
    let mut body = Vec::new();
    for part in parts {
        let mut head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
            part.name
        );
        if let Some(filename) = part.filename {
            let filename = filename
                .replace('\r', "%0D")
                .replace('\n', "%0A")
                .replace('"', "%22");
            head.push_str(&format!("; filename=\"{filename}\""));
        }
        head.push_str("\r\n");
        if let Some(content_type) = part.content_type {
            head.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        head.push_str("\r\n");
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_boundary_delimits_only_the_parts() {
        let file = b"\0enco-boundary\r\n--enco-boundary_";
        let (content_type, body) = multipart(&[
            FormPart {
                name: "payload",
                filename: None,
                content_type: Some("application/json"),
                data: b"{}",
            },
            FormPart {
                name: "file",
                filename: Some("plot.bin"),
                content_type: Some("application/octet-stream"),
                data: file,
            },
        ]);
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .expect("multipart content type");
        let delimiter = format!("\r\n--{boundary}");
        let delimiters = body
            .windows(delimiter.len())
            .filter(|window| *window == delimiter.as_bytes())
            .count();
        // One before the second part and one closing the body: the file bytes add none.
        assert_eq!(delimiters, 2);
        assert!(body.windows(file.len()).any(|window| window == file));
    }
}
