use crate::{Transcript, TranscriptItem};
use enco_core::*;
use std::collections::HashMap;

/// The entries that make up the conversation as it now stands.
///
/// The latest consumed backtrack keeps the conversation up to its target, then continues with
/// itself and every entry after it. The kept part honours the backtracks within it, so positions
/// can skip.
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
    let mut kept = conversation(&before[..kept(before, to)]);
    kept.extend(&entries[at..]);
    kept
}

/// Whether the Log ends with a backtrack to an input, which the next Run replies to again.
pub(crate) fn backtracked_to_input(entries: &[Entry]) -> bool {
    entries.last().and_then(backtrack).is_some_and(|to| {
        entries
            .binary_search_by_key(&to, |entry| entry.pos)
            .is_ok_and(|index| input(&entries[index]).is_some())
    })
}

/// The input an entry consumed; a backtrack is a command about the conversation, not an input.
pub(crate) fn input(entry: &Entry) -> Option<&Event> {
    match &entry.body {
        EntryBody::EventConsumed { event } if !is_backtrack(event) => Some(event),
        _ => None,
    }
}

/// Whether an input asks to take the conversation back.
pub(crate) fn is_backtrack(event: &Event) -> bool {
    matches!(event.body, EventBody::Backtrack { .. })
}

fn backtrack(entry: &Entry) -> Option<LogPos> {
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
