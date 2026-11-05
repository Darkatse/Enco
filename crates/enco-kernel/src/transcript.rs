use crate::{Transcript, TranscriptItem};
use enco_core::*;
use std::collections::HashMap;

/// The entries that make up the conversation as it now stands.
///
/// The latest consumed backtrack keeps the conversation up to its target, then continues with
/// itself and every entry after it; a backtrack to the start keeps no earlier entries. The kept
/// part honours the backtracks within it, so positions can skip.
pub(crate) fn conversation(entries: &[Entry]) -> Vec<&Entry> {
    let Some((at, to)) = entries
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, entry)| backtrack(entry).map(|to| (index, to)))
    else {
        return entries.iter().collect();
    };
    let before = &entries[..at];
    let mut kept = match to {
        Some(to) => conversation(&before[..kept(before, to)]),
        None => Vec::new(),
    };
    kept.extend(&entries[at..]);
    kept
}

/// Whether the Log ends with a backtrack to an input, which the next Run replies to again.
pub(crate) fn backtracked_to_input(entries: &[Entry]) -> bool {
    entries
        .last()
        .and_then(backtrack)
        .flatten()
        .is_some_and(|to| {
            entries
                .binary_search_by_key(&to, |entry| entry.pos)
                .is_ok_and(|index| input(&entries[index]).is_some())
        })
}

/// The ordinary input an entry consumed; commands about the conversation are not inputs.
pub(crate) fn input(entry: &Entry) -> Option<&Event> {
    match &entry.body {
        EntryBody::EventConsumed { event } if !event.body.is_command() => Some(event),
        _ => None,
    }
}

/// The latest completed reply request in a conversation, as its Attempt recorded it.
pub(crate) struct LastReply<'a> {
    /// Where the service's answer settled.
    pub settled: LogPos,
    /// The plan the request was resolved from.
    pub plan: &'a ContentHash,
    /// What the service reported for it.
    pub usage: &'a Usage,
    /// The call parameters that served it.
    pub settings: &'a ProviderSettings,
    /// The Provider generation that served it.
    pub provider: GenerationId,
}

/// The latest reply request the service answered, usable or not.
///
/// An answer measures the request; a failed or turned-away one measures nothing, and nothing in
/// the Log depends on it.
pub(crate) fn last_reply<'a>(conversation: &[&'a Entry]) -> Option<LastReply<'a>> {
    let mut answered = HashMap::new();
    for entry in conversation.iter().rev() {
        match &entry.body {
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { usage, .. },
                ..
            } => {
                answered.insert(*attempt, (entry.pos, usage));
            }
            EntryBody::AttemptStarted {
                attempt,
                purpose: AttemptPurpose::Reply,
                plan,
                provider,
                settings,
                ..
            } => {
                if let Some((settled, usage)) = answered.remove(attempt) {
                    return Some(LastReply {
                        settled,
                        plan,
                        usage,
                        settings,
                        provider: *provider,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the owner asked to compact the conversation and the request still holds.
///
/// Newest first, over the inputs not yet consumed and then the conversation: a request holds
/// until a Round ends, a compaction is recorded, or a backtrack leaves it behind. A halt leaves
/// it in place.
pub(crate) fn compaction_requested(conversation: &[&Entry], pending: &[Event]) -> bool {
    fn decides(body: &EventBody) -> Option<bool> {
        match body {
            EventBody::Compact => Some(true),
            EventBody::Backtrack { .. } => Some(false),
            _ => None,
        }
    }
    pending
        .iter()
        .rev()
        .find_map(|event| decides(&event.body))
        .or_else(|| {
            conversation
                .iter()
                .rev()
                .find_map(|entry| match &entry.body {
                    EntryBody::EventConsumed { event } => decides(&event.body),
                    EntryBody::RoundEnded { .. } | EntryBody::Compacted { .. } => Some(false),
                    _ => None,
                })
        })
        .unwrap_or(false)
}

/// Whether `conversation` has a complete Round after its latest compaction with visible
/// items to summarize.
pub(crate) fn has_compactable_round(conversation: &[&Entry]) -> bool {
    let transcript = project(conversation);
    transcript
        .items
        .first()
        .is_some_and(|first| transcript.round_ends.iter().any(|end| *end >= first.pos))
}

fn backtrack(entry: &Entry) -> Option<Option<LogPos>> {
    match &entry.body {
        EntryBody::EventConsumed {
            event:
                Event {
                    body: EventBody::Backtrack { to },
                    ..
                },
        } => Some(*to),
        _ => None,
    }
}

/// How many of `entries` a backtrack to `to` keeps.
///
/// It keeps them through the input it targets, otherwise through the end of the Round containing
/// `to`, with the Run's end when that Round was the Run's last.
fn kept(entries: &[Entry], to: LogPos) -> usize {
    let at = entries.partition_point(|entry| entry.pos < to);
    if entries
        .get(at)
        .is_some_and(|entry| entry.pos == to && input(entry).is_some())
    {
        return at + 1;
    }
    // A reply and its tool calls are one unit, so the cut falls where a Round ends. Backtracks are
    // consumed between Runs, so the target's Round has ended before the backtrack.
    let ended = |index: usize| {
        entries.get(index).is_some_and(|entry| {
            matches!(
                entry.body,
                EntryBody::RoundEnded { .. } | EntryBody::RunEnded { .. }
            )
        })
    };
    let Some(end) = (at..entries.len()).find(|index| ended(*index)) else {
        return entries.len();
    };
    let run_end = matches!(entries[end].body, EntryBody::RoundEnded { .. })
        && entries
            .get(end + 1)
            .is_some_and(|entry| matches!(entry.body, EntryBody::RunEnded { .. }));
    end + 1 + usize::from(run_end)
}

pub(crate) fn project(entries: &[&Entry]) -> Transcript {
    let compacted = entries.iter().rev().find_map(|e| match &e.body {
        EntryBody::Compacted { upto, summary, .. } => Some((*upto, summary.clone())),
        _ => None,
    });
    let mut transcript = Transcript {
        summary: compacted.as_ref().map(|(_, s)| s.clone()),
        ..Default::default()
    };
    let mut attempts = HashMap::new();
    let mut calls = HashMap::new();
    // Resolve call identities even before the compaction boundary; only visibility is filtered.
    for entry in entries {
        match &entry.body {
            EntryBody::AttemptStarted {
                attempt,
                purpose,
                provider,
                ..
            } => {
                attempts.insert(*attempt, (*purpose, *provider));
            }
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { message, .. },
                ..
            } if attempts
                .get(attempt)
                .is_some_and(|(purpose, _)| *purpose == AttemptPurpose::Reply) =>
            {
                for call in message.tool_calls() {
                    calls.insert(call.id, call);
                }
            }
            _ => {}
        }
        if compacted
            .as_ref()
            .is_some_and(|(upto, _)| entry.pos <= *upto)
        {
            continue;
        }
        let (purpose, generation, call) = match &entry.body {
            EntryBody::AttemptSettled { attempt, .. } => {
                let info = attempts.get(attempt);
                (
                    info.map(|(purpose, _)| *purpose),
                    info.map(|(_, generation)| *generation),
                    None,
                )
            }
            EntryBody::ToolCallSettled { call, .. } => (None, None, calls.get(call).copied()),
            _ => (None, None, None),
        };
        if let Some(message) = entry.body.canonical_message(purpose, call) {
            transcript.items.push(TranscriptItem {
                pos: entry.pos,
                message,
                generation,
                event: match &entry.body {
                    EntryBody::EventConsumed { event } => Some(event.clone()),
                    _ => None,
                },
            });
        }
        match &entry.body {
            EntryBody::RoundEnded { .. } => transcript.round_ends.push(entry.pos),
            EntryBody::RunEnded { end, .. } => transcript.run_ends.push((entry.pos, end.clone())),
            _ => {}
        }
    }
    transcript
}

#[cfg(test)]
mod tests;
