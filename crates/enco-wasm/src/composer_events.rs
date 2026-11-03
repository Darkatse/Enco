//! Projects recorded events into the composer's input without provider-owned extension contents.
use crate::bindings::{composers as wire, enco::plugin::types};
use enco_core::*;
use std::{collections::HashMap, path::PathBuf};

pub(super) fn pos(pos: LogPos) -> wire::LogPos {
    let LogPos { epoch, seq } = pos;
    wire::LogPos {
        epoch: epoch.0,
        seq: seq.0,
    }
}

pub(super) fn datetime(time: DateTime<Utc>) -> Result<types::Datetime, Failure> {
    Ok(types::Datetime {
        seconds: time
            .timestamp()
            .try_into()
            .map_err(|_| crate::convert::bad("recorded time precedes the Unix epoch"))?,
        nanoseconds: time.timestamp_subsec_nanos(),
    })
}

pub(super) fn event(
    event: &Event,
    paths: &HashMap<ContentHash, PathBuf>,
) -> Result<wire::Event, Failure> {
    // The Event belongs to the Session being composed, which the input already names.
    let Event {
        id,
        session: _,
        source,
        body,
        received_at,
    } = event;
    Ok(wire::Event {
        id: id.to_string(),
        received_at: datetime(*received_at)?,
        interactive: event.is_interactive(),
        source: event_source(source),
        body: event_body(body, paths)?,
    })
}

fn event_source(source: &EventSource) -> wire::EventSource {
    match source {
        EventSource::Parent => wire::EventSource::Parent,
        EventSource::Child(session) => wire::EventSource::Child(session_source(session)),
        EventSource::Cli => wire::EventSource::Cli,
        EventSource::Channel {
            protocol,
            account,
            conversation,
            sender,
            sender_name,
            owner,
            // Mapper evidence is a host concern and does not enter the composer contract.
            origin: _,
            // Composers do not point at platform messages; the contract does not carry the ID.
            message: _,
        } => wire::EventSource::Channel(wire::ChannelSource {
            protocol: protocol.clone(),
            account: account.clone(),
            conversation: conversation.clone(),
            sender: sender.clone(),
            sender_name: sender_name.clone(),
            owner: *owner,
        }),
        EventSource::Scheduler => wire::EventSource::Scheduler,
        EventSource::Registry => wire::EventSource::Registry,
    }
}

fn event_body(
    body: &EventBody,
    paths: &HashMap<ContentHash, PathBuf>,
) -> Result<wire::EventBody, Failure> {
    let body = match body {
        EventBody::UserMessage {
            text,
            attachments,
            unviewable,
            earlier,
            quote,
        } => wire::EventBody::UserMessage(wire::UserMessage {
            text: text.clone(),
            attachments: attachments
                .iter()
                .map(|attachment| attachment_ref(attachment, paths))
                .collect::<Result<_, _>>()?,
            unviewable: unviewable.iter().map(self::unviewable).collect(),
            earlier: earlier
                .iter()
                .map(earlier_message)
                .collect::<Result<_, _>>()?,
            quote: quote
                .as_ref()
                .map(|quote| quote_record(quote, paths))
                .transpose()?,
        }),
        EventBody::Reminder {
            schedule,
            due_at,
            skipped,
            text,
        } => wire::EventBody::Reminder(wire::Reminder {
            schedule: schedule.to_string(),
            due_at: datetime(*due_at)?,
            skipped: *skipped,
            text: text.clone(),
        }),
        EventBody::GenerationRolledBack {
            rollback:
                RolledBack {
                    plugin,
                    from,
                    to,
                    failure: fault,
                },
            text,
        } => wire::EventBody::GenerationRolledBack(wire::Rollback {
            plugin: plugin.to_string(),
            trial: from.0,
            replacement: to.map(|generation| generation.0),
            failure: crate::convert::failure_record(fault),
            text: text.clone(),
        }),
        EventBody::SessionMessage(Outgoing { text, file }) => {
            wire::EventBody::SessionMessage(wire::SessionMessage {
                text: text.clone(),
                file: file
                    .as_ref()
                    .map(|file| file_ref(file, paths))
                    .transpose()?,
            })
        }
        EventBody::ChildRunEnded { end } => {
            wire::EventBody::ChildRunEnded(crate::convert::run_end(end))
        }
        EventBody::Halt(value) => wire::EventBody::Halt(halt(value)),
        EventBody::Backtrack { to } => wire::EventBody::Backtrack(to.map(pos)),
        EventBody::Compact => {
            return Err(crate::convert::bad(
                "compaction request is not a transcript event",
            ));
        }
    };
    Ok(body)
}

fn earlier_message(item: &EarlierMessage) -> Result<wire::EarlierMessage, Failure> {
    let EarlierMessage {
        received_at,
        sender,
        sender_name,
        owner,
        text,
        attachments,
        quote,
        origin: _,
    } = item;
    Ok(wire::EarlierMessage {
        received_at: datetime(*received_at)?,
        sender: sender.clone(),
        sender_name: sender_name.clone(),
        owner: *owner,
        text: text.clone(),
        attachments: attachments.clone(),
        quote: quote.as_ref().map(earlier_quote).transpose()?,
    })
}

fn quote_record(
    quote: &Quote,
    paths: &HashMap<ContentHash, PathBuf>,
) -> Result<wire::Quote, Failure> {
    // Retrieval evidence is a host concern, like an input's mapping origin.
    let Quote {
        kind,
        messages,
        fetched: _,
    } = quote;
    let messages = match messages {
        Quoted::Messages(messages) => wire::Quoted::Messages(
            messages
                .iter()
                .map(|message| {
                    let QuotedMessage {
                        depth,
                        sender,
                        sender_name,
                        owner,
                        yours,
                        conversation_name,
                        sent_at,
                        text,
                        attachments,
                        unviewable,
                    } = message;
                    Ok(wire::QuotedMessage {
                        depth: *depth,
                        sender: sender.clone(),
                        sender_name: sender_name.clone(),
                        owner: *owner,
                        yours: *yours,
                        conversation_name: conversation_name.clone(),
                        sent_at: sent_at.map(datetime).transpose()?,
                        text: text.clone(),
                        attachments: attachments
                            .iter()
                            .map(|attachment| attachment_ref(attachment, paths))
                            .collect::<Result<_, _>>()?,
                        unviewable: unviewable.iter().map(self::unviewable).collect(),
                    })
                })
                .collect::<Result<_, Failure>>()?,
        ),
        Quoted::Unavailable(reason) => wire::Quoted::Unavailable(reason.clone()),
    };
    Ok(wire::Quote {
        kind: quote_kind(*kind),
        messages,
    })
}

fn earlier_quote(quote: &EarlierQuote) -> Result<wire::EarlierQuote, Failure> {
    let EarlierQuote { kind, messages } = quote;
    Ok(wire::EarlierQuote {
        kind: quote_kind(*kind),
        messages: messages.as_ref().map(earlier_quoted).transpose()?,
    })
}

fn earlier_quoted(quoted: &Quoted<String>) -> Result<wire::EarlierQuoted, Failure> {
    Ok(match quoted {
        Quoted::Messages(messages) => wire::EarlierQuoted::Messages(
            messages
                .iter()
                .map(|message| {
                    let QuotedMessage {
                        depth,
                        sender,
                        sender_name,
                        owner,
                        yours,
                        conversation_name,
                        sent_at,
                        text,
                        attachments,
                        unviewable,
                    } = message;
                    Ok(wire::EarlierQuotedMessage {
                        depth: *depth,
                        sender: sender.clone(),
                        sender_name: sender_name.clone(),
                        owner: *owner,
                        yours: *yours,
                        conversation_name: conversation_name.clone(),
                        sent_at: sent_at.map(datetime).transpose()?,
                        text: text.clone(),
                        attachments: attachments.clone(),
                        unviewable: unviewable.iter().map(self::unviewable).collect(),
                    })
                })
                .collect::<Result<_, Failure>>()?,
        ),
        Quoted::Unavailable(reason) => wire::EarlierQuoted::Unavailable(reason.clone()),
    })
}

fn quote_kind(kind: QuoteKind) -> types::QuoteKind {
    match kind {
        QuoteKind::Reply => types::QuoteKind::Reply,
        QuoteKind::Forward => types::QuoteKind::Forward,
    }
}

fn unviewable(item: &Unviewable) -> types::Unviewable {
    let Unviewable { what, reason } = item;
    types::Unviewable {
        what: what.clone(),
        reason: match reason {
            UnviewableReason::Unsupported => types::UnviewableReason::Unsupported,
            UnviewableReason::Unavailable(reason) => {
                types::UnviewableReason::Unavailable(reason.clone())
            }
        },
    }
}

pub(super) fn session_source(value: &SessionRef) -> wire::SessionSource {
    let SessionRef { id, name } = value;
    wire::SessionSource {
        id: id.to_string(),
        name: name.to_string(),
    }
}

pub(super) fn halt(value: &Halt) -> wire::Halt {
    let Halt {
        root: SessionRef { id: _, name },
        by,
    } = value;
    wire::Halt {
        root: name.to_string(),
        by: by
            .as_ref()
            .map(|SessionRef { id: _, name }| name.to_string()),
    }
}

pub(super) fn image_ref(
    image: &Image,
    paths: &HashMap<ContentHash, PathBuf>,
) -> Result<wire::ImageRef, Failure> {
    let Image {
        format,
        size,
        content,
    } = image;
    let path = paths
        .get(content)
        .ok_or_else(|| crate::convert::bad("a recorded image has no local path"))?;
    Ok(wire::ImageRef {
        format: (*format).into(),
        size: *size,
        content: content.to_string(),
        path: path.to_string_lossy().into_owned(),
    })
}

fn file_ref(file: &File, paths: &HashMap<ContentHash, PathBuf>) -> Result<wire::FileRef, Failure> {
    let File {
        name,
        size,
        content,
    } = file;
    let path = paths
        .get(content)
        .ok_or_else(|| crate::convert::bad("a recorded file has no local path"))?;
    Ok(wire::FileRef {
        name: name.clone(),
        size: *size,
        content: content.to_string(),
        path: path.to_string_lossy().into_owned(),
    })
}

fn attachment_ref(
    attachment: &Attachment,
    paths: &HashMap<ContentHash, PathBuf>,
) -> Result<wire::AttachmentRef, Failure> {
    match attachment {
        Attachment::Image(image) => image_ref(image, paths).map(wire::AttachmentRef::Image),
        Attachment::File(file) => file_ref(file, paths).map(wire::AttachmentRef::File),
    }
}
