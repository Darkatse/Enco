//! Projects recorded events into the composer's input without provider-owned extension contents.
use crate::bindings::{composers as wire, enco::plugin::types};
use enco_core::*;

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

pub(super) fn failure(failure: &Failure) -> types::Failure {
    let Failure {
        code,
        message,
        retryable,
    } = failure;
    types::Failure {
        code: code.clone(),
        message: message.clone(),
        retryable: *retryable,
    }
}

pub(super) fn event(event: &Event, located: &[wire::ImageRef]) -> Result<wire::Event, Failure> {
    // The Event belongs to the Session being composed, which the input already names.
    let Event {
        id,
        session: _,
        source,
        body,
        received_at,
    } = event;
    let source = match source {
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
    };
    let body = match body {
        EventBody::UserMessage {
            text,
            images,
            unviewable,
            earlier,
            quote,
        } => wire::EventBody::UserMessage(wire::UserMessage {
            text: text.clone(),
            images: locate(images, located)?,
            unviewable: unviewable.iter().map(self::unviewable).collect(),
            earlier: earlier
                .iter()
                .map(earlier_message)
                .collect::<Result<_, _>>()?,
            quote: quote
                .as_ref()
                .map(|quote| quote_record(quote, located))
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
            failure: failure(fault),
            text: text.clone(),
        }),
        EventBody::Backtrack { to } => wire::EventBody::Backtrack(pos(*to)),
    };
    Ok(wire::Event {
        id: id.to_string(),
        received_at: datetime(*received_at)?,
        source,
        body,
    })
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

/// The stored images arrive located in the recorded message that carries them.
fn locate(images: &[Image], located: &[wire::ImageRef]) -> Result<Vec<wire::ImageRef>, Failure> {
    images
        .iter()
        .map(|image| {
            let content = image.content.to_string();
            located
                .iter()
                .find(|located| located.content == content)
                .cloned()
                .ok_or_else(|| crate::convert::bad("a recorded image is missing from its message"))
        })
        .collect()
}

fn quote_record(quote: &Quote, located: &[wire::ImageRef]) -> Result<wire::Quote, Failure> {
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
                        images,
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
                        images: locate(images, located)?,
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
                        images,
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
                        images: images.clone(),
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
            UnviewableReason::TooLarge => types::UnviewableReason::TooLarge,
            UnviewableReason::Unavailable(reason) => {
                types::UnviewableReason::Unavailable(reason.clone())
            }
        },
    }
}

pub(super) fn run_end(end: &RunEnd) -> wire::RunEnd {
    match end {
        RunEnd::Completed => wire::RunEnd::Completed,
        RunEnd::BudgetExhausted => wire::RunEnd::BudgetExhausted,
        RunEnd::Interrupted => wire::RunEnd::Interrupted,
        RunEnd::Cancelled => wire::RunEnd::Cancelled,
        RunEnd::Failed { failure: value } => wire::RunEnd::Failed(failure(value)),
    }
}
