//! Where a Session's replies go: to the conversation of the owner's latest input, if that input
//! came through this connection.
//!
//! Each reply is sent once it is complete. Input through the CLI or another connection takes
//! replies away from this one; reminders and registry notices do not move them.
use super::{Cursor, Identity};
use enco_core::*;

/// Advance through settled messages and complete boundaries; leave unfinished replies for the next
/// wake.
pub(super) fn project(
    cursor: &mut Cursor,
    identity: &Identity,
    session: SessionId,
    entries: Vec<Entry>,
) -> Option<(Delivery, Outgoing)> {
    let mut target = cursor.target.clone();
    let mut reply_attempt = None;
    let mut reply = String::new();
    for entry in entries {
        let outgoing = match entry.body {
            EntryBody::EventConsumed { event } => {
                route(&mut target, identity, event.source);
                continue;
            }
            EntryBody::RoundStarted { .. } => {
                reply.clear();
                reply_attempt = None;
                continue;
            }
            EntryBody::AttemptStarted {
                attempt,
                purpose: AttemptPurpose::Reply,
                ..
            } => {
                reply_attempt = Some(attempt);
                continue;
            }
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { message, .. },
            } if Some(attempt) == reply_attempt => {
                reply = message.joined_text();
                continue;
            }
            EntryBody::ToolCallSettled {
                outgoing: Some(message),
                ..
            } => Some(message),
            EntryBody::RoundEnded { end, .. } => {
                matches!(end, RoundEnd::Replied).then(|| Outgoing {
                    text: std::mem::take(&mut reply),
                    file: None,
                })
            }
            EntryBody::RunEnded { end, .. } => (match end {
                RunEnd::Completed => None,
                RunEnd::Cancelled => Some("Run cancelled.".into()),
                RunEnd::Interrupted => {
                    Some("Run interrupted by a restart; check `enco log` for details.".into())
                }
                RunEnd::BudgetExhausted => Some("Run reached its Round limit.".into()),
                RunEnd::Failed { failure } => Some(format!("Run failed: {}", failure.message)),
            })
            .map(|text| Outgoing { text, file: None }),
            _ => continue,
        };
        cursor.processed = Some(entry.pos);
        cursor.target = target.clone();
        if let (Some(target), Some(outgoing)) = (target.clone(), outgoing) {
            return Some((
                Delivery {
                    session,
                    pos: entry.pos,
                    target,
                },
                outgoing,
            ));
        }
    }
    None
}

/// Interactive inputs move the reply destination; noninteractive events leave it in place.
pub(super) fn route(target: &mut Option<String>, identity: &Identity, source: EventSource) {
    match source {
        EventSource::Cli => *target = None,
        EventSource::Channel {
            protocol,
            account,
            conversation,
            ..
        } => {
            *target = (protocol == identity.protocol && account == identity.account)
                .then_some(conversation);
        }
        EventSource::Scheduler | EventSource::Registry => {}
    }
}
