use super::{Cursor, Identity};
use enco_core::*;

/// Advance through settled messages and complete boundaries; leave unfinished replies for the next wake.
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
                match event.source {
                    EventSource::Cli => target = None,
                    EventSource::Channel {
                        channel,
                        account,
                        conversation,
                        ..
                    } => {
                        target = (channel == identity.channel && account == identity.account)
                            .then_some(conversation);
                    }
                    EventSource::Scheduler | EventSource::Registry => {}
                }
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
