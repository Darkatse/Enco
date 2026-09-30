use super::{Cursor, Identity};
use enco_core::*;

/// Read complete boundaries only: an unfinished Round is re-read on the next wake.
pub(super) fn project(
    cursor: &mut Cursor,
    identity: &Identity,
    session: SessionId,
    entries: Vec<Entry>,
) -> Option<(Delivery, String)> {
    let mut target = cursor.target.clone();
    let mut reply_attempt = None;
    let mut reply = String::new();
    for entry in entries {
        let text = match entry.body {
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
                    EventSource::Scheduler => {}
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
            EntryBody::RoundEnded { end, .. } => {
                if matches!(end, RoundEnd::Replied) && !reply.trim().is_empty() {
                    Some(std::mem::take(&mut reply))
                } else {
                    None
                }
            }
            EntryBody::RunEnded { end, .. } => match end {
                RunEnd::Completed => None,
                RunEnd::Cancelled => Some("Run cancelled.".into()),
                RunEnd::Interrupted => {
                    Some("Run interrupted by a restart; check `enco log` for details.".into())
                }
                RunEnd::BudgetExhausted => Some("Run reached its Round limit.".into()),
                RunEnd::Failed { failure } => Some(format!("Run failed: {}", failure.message)),
            },
            _ => continue,
        };
        cursor.processed = Some(entry.pos);
        cursor.target = target.clone();
        if let (Some(target), Some(text)) = (target.clone(), text) {
            return Some((
                Delivery {
                    session,
                    pos: entry.pos,
                    target,
                },
                text,
            ));
        }
    }
    None
}
