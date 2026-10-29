//! Presents earlier, replied-to and forwarded messages with the same quoted lines.
use crate::composer::*;
use crate::{history, images};
use chrono_tz::Tz;
use enco_sdk::types::{Datetime, Failure, QuoteKind};

/// One quoted message as a note shows it.
struct View<'a> {
    depth: u8,
    sent_at: Option<&'a Datetime>,
    name: String,
    text: &'a str,
    attachments: Vec<String>,
}

/// The two quoted-message records share every field but how they keep images.
macro_rules! view {
    ($message:expr, $attachments:expr) => {
        View {
            depth: $message.depth,
            sent_at: $message.sent_at.as_ref(),
            name: quoted_name(
                $message.owner,
                $message.yours,
                $message.sender_name.as_deref(),
                $message.sender.as_deref(),
                $message.conversation_name.as_deref(),
            ),
            text: &$message.text,
            attachments: $attachments,
        }
    };
}

pub(super) fn sender_name<'a>(
    owner: bool,
    yours: bool,
    name: Option<&'a str>,
    sender: Option<&'a str>,
) -> &'a str {
    if yours {
        "You"
    } else if owner {
        "Owner"
    } else {
        name.or(sender).unwrap_or("Unknown sender")
    }
}

/// The sender, followed by the conversation when the message came from another one.
fn quoted_name(
    owner: bool,
    yours: bool,
    name: Option<&str>,
    sender: Option<&str>,
    conversation: Option<&str>,
) -> String {
    let anonymous = !owner && !yours && name.is_none() && sender.is_none();
    let name = sender_name(owner, yours, name, sender);
    match conversation {
        Some(conversation) if anonymous => conversation.into(),
        Some(conversation) => format!("{name} in {conversation}"),
        None => name.into(),
    }
}

/// An earlier attachment is only described, whatever it is.
fn attachment(what: &str) -> String {
    format!("[attachment: {what}]")
}

pub(super) fn incoming(quote: &Quote, speaker: &str, timezone: Tz) -> Result<String, Failure> {
    Ok(match &quote.messages {
        Quoted::Messages(messages) => {
            let views: Vec<_> = messages
                .iter()
                .map(|message| {
                    view!(
                        message,
                        images::descriptions(&message.images, &message.unviewable)
                    )
                })
                .collect();
            format!(
                "{speaker} {}:\n{}",
                relation(quote.kind, top_level(&views)),
                render(&views, 1, timezone)?
            )
        }
        Quoted::Unavailable(reason) => format!("{speaker} {}: {reason}", unavailable(quote.kind)),
    })
}

pub(super) fn earlier(message: &EarlierMessage, timezone: Tz) -> Result<String, Failure> {
    let quoted: Vec<_> = match message
        .quote
        .as_ref()
        .and_then(|quote| quote.messages.as_ref())
    {
        Some(EarlierQuoted::Messages(messages)) => messages
            .iter()
            .map(|message| {
                let attachments = message
                    .images
                    .iter()
                    .map(String::as_str)
                    .chain(message.unviewable.iter().map(|item| item.what.as_str()))
                    .map(attachment)
                    .collect();
                view!(message, attachments)
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut name = sender_name(
        message.owner,
        false,
        message.sender_name.as_deref(),
        message.sender.as_deref(),
    )
    .to_owned();
    // A reply names only its target, which is usually just above; a forward shows what it carried.
    let mut shown: &[View] = &[];
    if let Some(quote) = &message.quote {
        name.push_str(&earlier_relation(quote, &quoted));
        if quote.kind == QuoteKind::Forward {
            shown = &quoted;
        }
    }
    let own = View {
        depth: 0,
        sent_at: Some(&message.received_at),
        name,
        text: &message.text,
        attachments: message
            .attachments
            .iter()
            .map(String::as_str)
            .map(attachment)
            .collect(),
    };
    let mut text = render(std::slice::from_ref(&own), 1, timezone)?;
    if !shown.is_empty() {
        text.push('\n');
        text.push_str(&render(shown, 2, timezone)?);
    }
    Ok(text)
}

fn earlier_relation(quote: &EarlierQuote, quoted: &[View]) -> String {
    match (quote.kind, &quote.messages) {
        (QuoteKind::Reply, Some(EarlierQuoted::Messages(_))) => {
            let names: Vec<_> = quoted
                .iter()
                .filter(|view| view.depth == 0)
                .map(|view| view.name.as_str())
                .collect();
            format!(", replying to {}", names.join(", "))
        }
        (QuoteKind::Forward, Some(EarlierQuoted::Messages(_))) => {
            format!(" {}", relation(QuoteKind::Forward, top_level(quoted)))
        }
        (kind, messages) => {
            let relation = match kind {
                QuoteKind::Reply => ", replying to a message not shown",
                QuoteKind::Forward => " forwards messages not shown",
            };
            match messages {
                Some(EarlierQuoted::Unavailable(reason)) => format!("{relation} ({reason})"),
                _ => relation.into(),
            }
        }
    }
}

fn top_level(views: &[View]) -> usize {
    views.iter().filter(|view| view.depth == 0).count()
}

fn relation(kind: QuoteKind, count: usize) -> String {
    let verb = match kind {
        QuoteKind::Reply => "replies to",
        QuoteKind::Forward => "forwards",
    };
    if count == 1 {
        format!("{verb} this message")
    } else {
        format!("{verb} these {count} messages")
    }
}

fn unavailable(kind: QuoteKind) -> &'static str {
    match kind {
        QuoteKind::Reply => "replies to a message that could not be shown",
        QuoteKind::Forward => "forwards messages that could not be shown",
    }
}

/// Each message starts with its time and sender; a nested forward is one `>` deeper.
fn render(views: &[View], base: usize, timezone: Tz) -> Result<String, Failure> {
    let mut rendered = Vec::new();
    for (index, view) in views.iter().enumerate() {
        let children = views[index + 1..]
            .iter()
            .take_while(|child| child.depth > view.depth)
            .filter(|child| u16::from(child.depth) == u16::from(view.depth) + 1)
            .count();
        let mut header = match view.sent_at {
            Some(time) => format!(
                "[{}] ",
                history::instant(time)?
                    .with_timezone(&timezone)
                    .format(history::LOCAL_TIME)
            ),
            None => String::new(),
        };
        header.push_str(&view.name);
        if children > 0 {
            header.push_str(&format!(" {}", relation(QuoteKind::Forward, children)));
        }
        header.push(':');
        let mut lines = view
            .text
            .lines()
            .map(str::to_owned)
            .chain(view.attachments.iter().cloned());
        if let Some(first) = lines.next() {
            header.push(' ');
            header.push_str(&first);
        }
        let prefix = ">".repeat(base + usize::from(view.depth));
        rendered.push(format!("{prefix} {header}"));
        rendered.extend(lines.map(|line| format!("{prefix} {line}")));
    }
    Ok(rendered.join("\n"))
}
