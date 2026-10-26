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

pub(super) fn datetime(time: DateTime<Utc>) -> Result<wire::Datetime, Failure> {
    Ok(wire::Datetime {
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

pub(super) fn event(event: &Event, images: Vec<wire::ImageRef>) -> Result<wire::Event, Failure> {
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
            channel,
            account,
            conversation,
            sender,
            // Composers do not point at platform messages; the contract does not carry the ID.
            message: _,
        } => wire::EventSource::Channel(wire::ChannelSource {
            channel: channel.clone(),
            account: account.clone(),
            conversation: conversation.clone(),
            sender: sender.clone(),
        }),
        EventSource::Scheduler => wire::EventSource::Scheduler,
        EventSource::Registry => wire::EventSource::Registry,
    };
    let body = match body {
        // The stored images arrive located, with the recorded message that carries them.
        EventBody::UserMessage {
            text,
            images: _,
            unviewable,
        } => wire::EventBody::UserMessage(wire::UserMessage {
            text: text.clone(),
            images,
            unviewable: unviewable.iter().map(self::unviewable).collect(),
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

fn unviewable(item: &Unviewable) -> wire::Unviewable {
    let Unviewable { what, reason } = item;
    wire::Unviewable {
        what: what.clone(),
        reason: match reason {
            UnviewableReason::Unsupported => wire::UnviewableReason::Unsupported,
            UnviewableReason::TooLarge => wire::UnviewableReason::TooLarge,
            UnviewableReason::Unavailable(reason) => {
                wire::UnviewableReason::Unavailable(reason.clone())
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
