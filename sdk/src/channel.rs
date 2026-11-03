//! What every channel mapper shares: commands, decoded content, the activity card's wording and
//! `multipart/form-data` uploads.
//!
//! Each protocol marks commands and lays the card out its own way; the names, meanings and words
//! are the same everywhere. [`export!`](crate::export) gives a channel plugin these same types, so
//! name them from here.
pub use crate::bindings::exports::enco::plugin::channel::{
    Action, ActivityEpilogue, ActivityUpdate, ActivityView, Attachment, CommandKind, Content,
    ConversationName, Quote, Quoted, QuotedMessage, ToolActivity, ToolState,
};
pub use crate::types::RunEnd;
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
            Self::Halt => "halt",
            Self::Archive => "archive",
        }
    }

    /// The command with this exact, case-sensitive name; unknown names are ordinary text.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Session,
            Self::Cancel,
            Self::Backtrack,
            Self::Everyone,
            Self::Halt,
            Self::Archive,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }

    /// What the command asks for with `argument`, compared without surrounding whitespace;
    /// unknown arguments are ordinary text.
    pub fn action(self, argument: &str) -> Option<Action> {
        match (self, argument.trim()) {
            (Self::Session, "") => Some(Action::Sessions),
            (Self::Session, name) => Some(Action::SelectSession(name.into())),
            (Self::Halt, "") => Some(Action::Halt(None)),
            (Self::Halt, name) => Some(Action::Halt(Some(name.into()))),
            (Self::Archive, "") => Some(Action::Archive(None)),
            (Self::Archive, name) => Some(Action::Archive(Some(name.into()))),
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

impl ActivityEpilogue {
    /// The epilogue's line: the outcome's mark, the time and Rounds the Run took, then ` · ` and
    /// its reply's first line.
    ///
    /// A reply that was not sent is marked `not sent:`. The time keeps its units, such as `45s`, `7m 3s` or `2h 5m`: on a line of its own, `7:03`
    /// would read as a time of day.
    pub fn text(&self) -> String {
        let mark = match self.end {
            RunEnd::Completed => "✓",
            RunEnd::BudgetExhausted => "✗ max rounds",
            RunEnd::Interrupted => "? interrupted",
            RunEnd::Cancelled => "✗ cancelled",
            RunEnd::Backtracked => "✗ backtracked",
            RunEnd::Failed(_) => "✗ failed",
            RunEnd::Halted => "✗ halted",
        };
        let rounds = if self.rounds == 1 {
            "1 round".into()
        } else {
            format!("{} rounds", self.rounds)
        };
        let mut text = format!("{mark} {}, {rounds}", duration(self.elapsed_seconds));
        match (self.unsent, self.excerpt.is_empty()) {
            (true, true) => text.push_str(" · not sent"),
            (true, false) => text.push_str(&format!(" · not sent: {}", self.excerpt)),
            (false, false) => text.push_str(&format!(" · {}", self.excerpt)),
            (false, true) => {}
        }
        text
    }
}

/// `seconds` in its two largest units, leaving out a zero second unit: `45s`, `7m`, `7m 3s`,
/// `2h 5m`.
fn duration(seconds: u32) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let (major, minor) = match (hours, minutes) {
        (0, 0) => return format!("{seconds}s"),
        (0, _) => ((minutes, 'm'), (seconds, 's')),
        _ => ((hours, 'h'), (minutes, 'm')),
    };
    match minor {
        (0, _) => format!("{}{}", major.0, major.1),
        _ => format!("{}{} {}{}", major.0, major.1, minor.0, minor.1),
    }
}

/// A child's full Session name joined with ` › `, shortened to `max_chars` characters with an ellipsis.
///
/// `max_chars` must be positive.
pub fn child_title(session: &[String], max_chars: usize) -> String {
    let title = session.join(" › ");
    if title.chars().nth(max_chars).is_some() {
        title.chars().take(max_chars - 1).chain(['…']).collect()
    } else {
        title
    }
}

impl ActivityView {
    /// The activity card's text: `Working` and the elapsed `m:ss`, then a line per tool.
    ///
    /// Each tool line is a mark, the tool's name and, after ` · `, its arguments when it has
    /// any; the marks are `…` running, `✓` ok, `✗` failed and `?` unknown.
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
            if !tool.arguments.is_empty() {
                text.push_str(&format!(" · {}", tool.arguments));
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

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn epilogues_tell_outcomes_apart_and_mark_an_unsent_reply() {
        let epilogue = |end, unsent, excerpt: &str| ActivityEpilogue {
            elapsed_seconds: 45,
            rounds: 1,
            end,
            unsent,
            excerpt: excerpt.into(),
        };
        let ends = [
            RunEnd::Completed,
            RunEnd::BudgetExhausted,
            RunEnd::Interrupted,
            RunEnd::Cancelled,
            RunEnd::Backtracked,
            RunEnd::Failed(crate::protocol::failure("test", "failure", false)),
            RunEnd::Halted,
        ];
        let lines: std::collections::BTreeSet<_> = ends
            .into_iter()
            .map(|end| epilogue(end, false, "").text())
            .collect();
        assert_eq!(lines.len(), 7);
        let sent = epilogue(RunEnd::Completed, false, "找到三篇相关论文").text();
        assert!(sent.ends_with("找到三篇相关论文") && !sent.contains("not sent"));
        let unsent = epilogue(RunEnd::Completed, true, "暂时没有新消息").text();
        assert!(unsent.contains("not sent") && unsent.ends_with("暂时没有新消息"));
        assert!(
            epilogue(RunEnd::Completed, true, "")
                .text()
                .contains("not sent")
        );
    }

    #[test]
    fn long_runs_count_hours_instead_of_minutes_past_sixty() {
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(7 * 60 + 3), "7m 3s");
        assert_eq!(duration(2 * 3600 + 5 * 60 + 3), "2h 5m");
        assert_eq!(duration(3600), "1h");
    }

    #[test]
    fn child_titles_keep_the_full_path_and_truncate_unicode_on_character_boundaries() {
        let names = vec!["main".into(), "研究".into(), "papers".into()];
        assert_eq!(child_title(&names, 100), "main › 研究 › papers");
        assert_eq!(child_title(&names, 10), "main › 研究…");
        assert_eq!(child_title(&["😀".repeat(100)], 100), "😀".repeat(100));
        assert_eq!(
            child_title(&["😀".repeat(101)], 100),
            format!("{}…", "😀".repeat(99))
        );
    }
}
