//! Where a Session's replies go on this connection: core's delivery projection, resumed from the
//! connection's cursor.
//!
//! Only conversations of this protocol account receive replies, each once it is complete. Input
//! through the CLI, another connection or a parent Session takes replies away from this one;
//! inputs that ask for no reply do not move them.
use super::{Cursor, Identity};
use enco_core::*;

/// Advance through completed boundaries after the cursor; stop at the first one this connection
/// delivers.
pub(super) fn project(
    cursor: &mut Cursor,
    identity: &Identity,
    session: SessionId,
    entries: Vec<Entry>,
) -> Option<(Delivery, Outgoing)> {
    let mut projection = DeliveryProjection::resume(cursor.destination.clone());
    for entry in entries {
        let Some(boundary) = projection.step(&entry) else {
            continue;
        };
        cursor.processed = Some(entry.pos);
        cursor.destination = boundary.destination.clone();
        let target = boundary
            .destination
            .as_ref()
            .and_then(|destination| channel_target(destination, identity))
            .map(str::to_owned);
        if let (Some(target), Some(content)) = (target, boundary.deliverable) {
            return Some((
                Delivery {
                    session,
                    pos: entry.pos,
                    target,
                },
                outgoing(content),
            ));
        }
    }
    None
}

/// The cursor once a delivery settles: replies were going to its conversation.
pub(super) fn delivered(identity: &Identity, delivery: &Delivery) -> Cursor {
    Cursor {
        processed: Some(delivery.pos),
        destination: Some(ReplyDestination::Channel {
            protocol: identity.protocol.clone(),
            account: identity.account.clone(),
            conversation: delivery.target.clone(),
        }),
    }
}

/// The conversation on this protocol account that a destination names, if it names one.
pub(super) fn channel_target<'a>(
    destination: &'a ReplyDestination,
    identity: &Identity,
) -> Option<&'a str> {
    match destination {
        ReplyDestination::Channel {
            protocol,
            account,
            conversation,
        } if *protocol == identity.protocol && *account == identity.account => Some(conversation),
        ReplyDestination::Channel { .. } | ReplyDestination::Cli | ReplyDestination::Parent => None,
    }
}

/// The message the owner reads for what a Session handed over.
fn outgoing(content: Deliverable) -> Outgoing {
    let text = match content {
        Deliverable::Message(outgoing) => return outgoing,
        Deliverable::Reply(text) => text,
        Deliverable::RunEnded(end) => match end {
            RunEnd::Cancelled => "Run cancelled.".into(),
            RunEnd::Backtracked => "Run stopped to go back to an earlier message.".into(),
            RunEnd::Interrupted => {
                "Run interrupted by a restart; check `enco log` for details.".into()
            }
            RunEnd::BudgetExhausted => "Run reached its Round limit.".into(),
            RunEnd::Failed { failure } => format!("Run failed: {}", failure.message),
            RunEnd::Halted => "Run halted.".into(),
            RunEnd::Completed => "Run completed.".into(),
        },
    };
    Outgoing { text, file: None }
}
