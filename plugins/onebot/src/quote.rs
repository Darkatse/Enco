//! Fetching the messages OneBot reply and merged-forward segments name, replayed from the
//! responses so far so that the same responses always give the same step.
use crate::{
    channel::{Fault, FetchStep, Request, Response},
    content,
    incoming::Sender,
    wire::{self, Target},
};
use enco_sdk::{
    channel::QuotedMessage,
    types::{Datetime, Unviewable},
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The messages `target` names, given the `responses` to the requests sent so far; one fetch sends
/// at most `limit` requests.
///
/// A reply to a merged forward shows the forwarded messages, not the message that carries them. A
/// fatal or retryable failure stops the fetch. Any other failure to read the replied message or the
/// outermost forward makes the quote unavailable; for a nested forward, only that forward.
pub(super) fn fetch(
    target: Target,
    responses: Vec<Response>,
    limit: u32,
) -> Result<FetchStep, Fault> {
    let mut responses = Responses {
        rest: responses.into_iter(),
        sent: 0,
        // Each layer of forwards takes a request, so this keeps every depth within a `u8`.
        limit: limit.min(u32::from(u8::MAX)),
    };
    let root = match target {
        Target::Forward(id) => id,
        Target::Reply(id) => {
            let request = json!({"message_id": id});
            let request = wire::request("get_msg", request, wire::ACTION_TIMEOUT_MS);
            let data = match responses.answer(request)? {
                Answer::Data(data) => data,
                Answer::Pending(request) => return Ok(FetchStep::Request(request)),
                Answer::Unavailable(reason) => return Ok(FetchStep::Unavailable(reason)),
            };
            let node = node(data, 0);
            match node.forward {
                Some(forward) => forward,
                None => return Ok(FetchStep::Messages(vec![node.message])),
            }
        }
    };
    expand(root, responses)
}

/// The responses to one fetch's requests so far, in the order the requests were sent.
struct Responses {
    rest: std::vec::IntoIter<Response>,
    sent: u32,
    limit: u32,
}

/// What became of a request.
enum Answer {
    /// OneBot answered with this data.
    Data(Value),
    /// It has no response yet, so the host sends it next.
    Pending(Request),
    /// It has no answer, for this reason: OneBot refused it, or the fetch used up its requests.
    Unavailable(String),
}

impl Responses {
    /// What became of `request`, the next one this fetch sends.
    fn answer(&mut self, request: Request) -> Result<Answer, Fault> {
        if self.sent == self.limit {
            let reason = format!("the {}-request limit of one fetch was reached", self.limit);
            return Ok(Answer::Unavailable(reason));
        }
        let Some(response) = self.rest.next() else {
            return Ok(Answer::Pending(request));
        };
        self.sent += 1;
        match wire::answer(response) {
            Ok(data) => Ok(Answer::Data(data)),
            // These keep the recovery the protocol gives: stopping or retrying the connection.
            Err(fault) if fault.fatal || fault.failure.retryable => Err(fault),
            Err(fault) => Ok(Answer::Unavailable(fault.failure.message)),
        }
    }
}

/// A merged forward met while expanding; its index among them is the order of its request.
struct Forward {
    id: String,
    depth: u8,
}

/// A message from a merged forward, with the index of the forward it carries, if any.
struct Entry {
    message: QuotedMessage,
    nested: Option<usize>,
}

#[derive(Deserialize)]
struct Forwarded {
    messages: Vec<Value>,
}

/// Expands the merged forward `root` and the forwards inside it layer by layer from the outside
/// in, then lists their messages in document order.
fn expand(root: String, mut responses: Responses) -> Result<FetchStep, Fault> {
    let mut forwards = vec![Forward { id: root, depth: 0 }];
    // Each forward's messages, or why it was not expanded, by index.
    let mut expanded: Vec<Result<Vec<Entry>, String>> = vec![];
    while let Some(forward) = forwards.get(expanded.len()) {
        let depth = forward.depth;
        let request = json!({"id": forward.id});
        let request = wire::request("get_forward_msg", request, wire::ACTION_TIMEOUT_MS);
        let entries = match responses.answer(request)? {
            Answer::Pending(request) => return Ok(FetchStep::Request(request)),
            Answer::Unavailable(reason) => Err(reason),
            Answer::Data(data) => match Forwarded::deserialize(data) {
                Err(error) => Err(format!("invalid forwarded messages: {error}")),
                Ok(forwarded) if forwarded.messages.is_empty() => {
                    Err("the merged forward has no messages".into())
                }
                Ok(forwarded) => Ok(forwarded
                    .messages
                    .into_iter()
                    .map(|value| {
                        let node = node(value, depth);
                        let nested = node.forward.map(|id| {
                            forwards.push(Forward {
                                id,
                                depth: depth + 1,
                            });
                            forwards.len() - 1
                        });
                        Entry {
                            message: node.message,
                            nested,
                        }
                    })
                    .collect()),
            },
        };
        expanded.push(entries);
    }
    match std::mem::replace(&mut expanded[0], Ok(vec![])) {
        Err(reason) => Ok(FetchStep::Unavailable(reason)),
        Ok(entries) => {
            let mut messages = vec![];
            list(entries, &mut expanded, &mut messages);
            Ok(FetchStep::Messages(messages))
        }
    }
}

/// Adds `entries` to `messages`, each nested forward's messages right after the message that
/// carries it.
///
/// A forward that was not expanded is unviewable on that message.
fn list(
    entries: Vec<Entry>,
    expanded: &mut [Result<Vec<Entry>, String>],
    messages: &mut Vec<QuotedMessage>,
) {
    for Entry {
        mut message,
        nested,
    } in entries
    {
        // Each forward is carried by one message, so taking its entries leaves nothing behind.
        match nested.map(|index| std::mem::replace(&mut expanded[index], Ok(vec![]))) {
            None => messages.push(message),
            Some(Ok(entries)) => {
                messages.push(message);
                list(entries, expanded, messages);
            }
            Some(Err(reason)) => {
                let unexpanded = Unviewable::unavailable("forwarded messages", reason);
                message.unviewable.push(unexpanded);
                messages.push(message);
            }
        }
    }
}

/// A message OneBot returned for a quote.
#[derive(Deserialize)]
struct Shown {
    user_id: u64,
    #[serde(default)]
    sender: Sender,
    time: u64,
    message: Vec<Value>,
}

/// A message `depth` forwards deep, with the ID of the merged forward it carries.
struct Node {
    message: QuotedMessage,
    forward: Option<String>,
}

/// The message OneBot returned as `value`; its own reply is not shown.
fn node(value: Value, depth: u8) -> Node {
    let shown = match Shown::deserialize(value) {
        Ok(shown) => shown,
        Err(error) => {
            let invalid = Unviewable::unavailable(
                "quoted message",
                format!("invalid quoted message: {error}"),
            );
            return Node {
                message: QuotedMessage::invalid(depth, invalid),
                forward: None,
            };
        }
    };
    let segments = content::decode(&shown.message, None);
    let mut message = QuotedMessage::new(depth, segments.content);
    message.sender = Some(shown.user_id.to_string());
    message.sender_name = shown.sender.name();
    message.sent_at = Some(Datetime {
        seconds: shown.time,
        nanoseconds: 0,
    });
    let forward = match segments.quote {
        Some(Target::Forward(id)) => Some(id),
        Some(Target::Reply(_)) | None => None,
    };
    Node { message, forward }
}
