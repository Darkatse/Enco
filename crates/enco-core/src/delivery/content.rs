use crate::{
    AttemptId, AttemptPurpose, AttemptResult, Entry, EntryBody, Outgoing, ReplyDestination,
    RoundEnd, RunEnd,
};

/// What a Session hands to its reply destination at a delivery boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum Deliverable {
    /// The reply that ended a Round.
    Reply(String),
    /// A `message_send` payload, handed over as its call settles.
    Message(Outgoing),
    /// How a Run that did not complete ended, handed over instead of a reply; never `Completed`.
    RunEnded(RunEnd),
}

/// A Log entry at which a Session hands content to its reply destination, or would if it had
/// any.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    /// Where replies go at this entry.
    pub destination: Option<ReplyDestination>,
    /// Content to hand over; absent for a Round that ended without a reply and for a Run that
    /// completed.
    pub deliverable: Option<Deliverable>,
}

/// Folds a Session's Log, in order, into what it hands to its reply destination.
///
/// This is the only definition of that content: the channel shell and the delivery to a parent
/// both fold through it. A Round that replies has no tool calls, so no reply is in progress at a
/// boundary and resuming after one needs only the destination there.
#[derive(Debug, Clone, Default)]
pub struct DeliveryProjection {
    destination: Option<ReplyDestination>,
    reply_attempt: Option<AttemptId>,
    reply: String,
}

impl DeliveryProjection {
    /// Resume folding after a boundary that recorded this destination.
    pub fn resume(destination: Option<ReplyDestination>) -> Self {
        Self {
            destination,
            ..Self::default()
        }
    }

    /// Where replies go after the entries folded so far: the latest input that asked for a reply.
    pub fn destination(&self) -> Option<&ReplyDestination> {
        self.destination.as_ref()
    }

    /// Fold the next entry; returns the boundary it completes, if it is one.
    pub fn step(&mut self, entry: &Entry) -> Option<Boundary> {
        let deliverable = match &entry.body {
            EntryBody::EventConsumed { event } => {
                if let Some(next) = event.reply_destination() {
                    self.destination = Some(next);
                }
                return None;
            }
            EntryBody::RoundStarted { .. } => {
                self.reply.clear();
                self.reply_attempt = None;
                return None;
            }
            EntryBody::AttemptStarted {
                attempt,
                purpose: AttemptPurpose::Reply,
                ..
            } => {
                self.reply_attempt = Some(*attempt);
                return None;
            }
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { message, .. },
                ..
            } if Some(*attempt) == self.reply_attempt => {
                self.reply = message.joined_text();
                return None;
            }
            EntryBody::ToolCallSettled {
                outgoing: Some(outgoing),
                ..
            } => Some(Deliverable::Message(outgoing.clone())),
            EntryBody::RoundEnded { end, .. } => matches!(end, RoundEnd::Replied)
                .then(|| Deliverable::Reply(std::mem::take(&mut self.reply))),
            EntryBody::RunEnded { end, .. } => {
                unfinished(end).then(|| Deliverable::RunEnded(end.clone()))
            }
            _ => return None,
        };
        Some(Boundary {
            destination: self.destination.clone(),
            deliverable,
        })
    }
}

/// Whether a Run ended without completing; its end is handed over instead of a reply.
fn unfinished(end: &RunEnd) -> bool {
    !matches!(end, RunEnd::Completed)
}

/// Whether an entry answers the inputs consumed before it: a Round's reply, or how a Run that did
/// not complete ended.
pub(super) fn answers(body: &EntryBody) -> bool {
    match body {
        EntryBody::RoundEnded {
            end: RoundEnd::Replied,
            ..
        } => true,
        EntryBody::RunEnded { end, .. } => unfinished(end),
        _ => false,
    }
}
