use crate::{Transcript, TranscriptItem};
use enco_core::*;
use std::collections::HashMap;

/// The entries that make up the conversation as it now stands.
///
/// The latest consumed backtrack keeps the conversation up to its target, then continues with
/// itself and every entry after it; a backtrack to the start keeps no earlier entries. The kept
/// part honours the backtracks within it, so positions can skip.
pub(crate) fn conversation(entries: &[Entry]) -> Vec<&Entry> {
    let mut end = entries.len();
    let mut parts = Vec::new();
    for passed in passed(entries) {
        parts.push(&entries[passed.at..end]);
        end = passed.kept;
    }
    parts.push(&entries[..end]);
    parts.into_iter().rev().flatten().collect()
}

/// How far back `entries` must be read for the conversation from `reach` on to be what the whole
/// Log gives, so that every read over it from there is too.
///
/// `reach` is how far back the reads over the conversation reach; `None` when they need all of
/// it, which only a backtrack to the start bounds. Each backtrack the conversation passes through
/// from there keeps the entries up to its target, so the target is read as well.
pub(crate) fn conversation_reach(
    entries: &[Entry],
    conversation: &[&Entry],
    reach: Option<LogPos>,
) -> Option<LogPos> {
    let mut reach = reach.or_else(|| {
        conversation
            .first()
            .filter(|first| backtrack(first) == Some(None))
            .map(|first| first.pos)
    })?;
    for passed in passed(entries) {
        if entries[passed.at].pos < reach {
            break;
        }
        if let Some(to) = passed.to {
            reach = reach.min(to);
        }
    }
    Some(reach)
}

/// Whether the Log owes a reply to an input it was taken back to, which the next Run gives.
///
/// It does when its latest entry that bears on replies is a backtrack to an input. Commands that
/// leave the inputs, compaction requests and those acting on the Session, leave the reply owed.
pub(crate) fn backtracked_to_input(entries: &[Entry]) -> bool {
    owed_target(entries).is_some_and(|to| {
        entries
            .binary_search_by_key(&to, |entry| entry.pos)
            .is_ok_and(|index| input(&entries[index]).is_some())
    })
}

/// How far back `backtracked_to_input` reads: to the target of the backtrack it finds.
///
/// Otherwise it reads to the entry it stops at, and needs all it was given while every entry
/// leaves a reply owed.
pub(crate) fn backtracked_to_input_reach(entries: &[Entry]) -> Option<LogPos> {
    owed_target(entries).or(bearing_on_replies(entries).map(|entry| entry.pos))
}

fn owed_target(entries: &[Entry]) -> Option<LogPos> {
    bearing_on_replies(entries).and_then(backtrack).flatten()
}

/// The latest entry that is not a consumed command leaving the inputs as they were.
fn bearing_on_replies(entries: &[Entry]) -> Option<&Entry> {
    entries.iter().rev().find(|entry| {
        !matches!(&entry.body, EntryBody::EventConsumed { event } if event.body.leaves_inputs())
    })
}

/// The ordinary input an entry consumed; a command is not one.
fn input(entry: &Entry) -> Option<&Event> {
    match &entry.body {
        EntryBody::EventConsumed { event } if !event.body.is_command() => Some(event),
        _ => None,
    }
}

/// The ordinary input the conversation consumed last, and where.
pub(crate) fn latest_input<'a>(conversation: &[&'a Entry]) -> Option<(LogPos, &'a Event)> {
    conversation
        .iter()
        .rev()
        .find_map(|entry| input(entry).map(|event| (entry.pos, event)))
}

/// How far back `latest_input` reads: to that input; `None` when the conversation has none.
pub(crate) fn latest_input_reach(conversation: &[&Entry]) -> Option<LogPos> {
    latest_input(conversation).map(|(pos, _)| pos)
}

/// Whether the conversation consumed an ordinary input since its latest Round ended.
pub(crate) fn new_input(conversation: &[&Entry]) -> bool {
    let start = latest_round_end(conversation).map_or(0, |end| end + 1);
    conversation[start..]
        .iter()
        .any(|entry| input(entry).is_some())
}

/// How far back `new_input` reads: to the latest Round end; `None` when no Round has ended.
pub(crate) fn new_input_reach(conversation: &[&Entry]) -> Option<LogPos> {
    latest_round_end(conversation).map(|end| conversation[end].pos)
}

fn latest_round_end(conversation: &[&Entry]) -> Option<usize> {
    conversation
        .iter()
        .rposition(|entry| matches!(entry.body, EntryBody::RoundEnded { .. }))
}

/// The latest completed reply request in a conversation, as its Attempt recorded it.
pub(crate) struct LastReply<'a> {
    /// Where the request started.
    pub started: LogPos,
    /// Where the service's answer settled.
    pub settled: LogPos,
    /// The plan the request was resolved from.
    pub plan: &'a ContentHash,
    /// What the service reported for it.
    pub usage: &'a Usage,
    /// The call parameters that served it.
    pub settings: &'a ProviderSettings,
    /// The Provider code that served it.
    pub provider: &'a CodeRef,
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
                        started: entry.pos,
                        settled,
                        plan,
                        usage,
                        settings,
                        provider,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

/// How far back `last_reply` reads: to where that request started; `None` when the conversation
/// has no answered reply request.
pub(crate) fn last_reply_reach(conversation: &[&Entry]) -> Option<LogPos> {
    last_reply(conversation).map(|last| last.started)
}

/// Whether the owner asked to compact the conversation and the request still holds.
///
/// Newest first, over the inputs not yet consumed and then the conversation: a request holds
/// until a Round ends, a compaction is recorded, or a backtrack leaves it behind. A halt leaves
/// it in place.
pub(crate) fn compaction_requested(conversation: &[&Entry], pending: &[Event]) -> bool {
    pending
        .iter()
        .rev()
        .find_map(|event| request_decided(&event.body))
        .or_else(|| request_decision(conversation).map(|(_, requested)| requested))
        .unwrap_or(false)
}

/// How far back `compaction_requested` reads the conversation: to the latest entry that decides
/// the request; `None` when none does.
pub(crate) fn compaction_requested_reach(conversation: &[&Entry]) -> Option<LogPos> {
    request_decision(conversation).map(|(pos, _)| pos)
}

fn request_decision(conversation: &[&Entry]) -> Option<(LogPos, bool)> {
    conversation.iter().rev().find_map(|entry| {
        match &entry.body {
            EntryBody::EventConsumed { event } => request_decided(&event.body),
            EntryBody::RoundEnded { .. } | EntryBody::Compacted { .. } => Some(false),
            _ => None,
        }
        .map(|requested| (entry.pos, requested))
    })
}

fn request_decided(body: &EventBody) -> Option<bool> {
    match body {
        EventBody::Compact => Some(true),
        EventBody::Backtrack { .. } => Some(false),
        _ => None,
    }
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

/// The target of the backtrack `entry` consumed, if it consumed one; a target of `None` returns
/// to the start.
pub(crate) fn backtrack(entry: &Entry) -> Option<Option<LogPos>> {
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

/// A consumed backtrack the conversation passes through.
struct Passed {
    /// Its index in the entries.
    at: usize,
    /// Its target; `None` returns to the start.
    to: Option<LogPos>,
    /// How many of the entries before it the conversation keeps.
    kept: usize,
}

/// The backtracks the conversation passes through, latest first: the latest consumed backtrack,
/// then the latest within what it keeps, and so on.
fn passed(entries: &[Entry]) -> impl Iterator<Item = Passed> {
    let mut end = entries.len();
    std::iter::from_fn(move || {
        let (at, to) = entries[..end]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, entry)| backtrack(entry).map(|to| (index, to)))?;
        end = to.map_or(0, |to| kept(&entries[..at], to));
        Some(Passed { at, to, kept: end })
    })
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

/// How far back `project` reads the conversation: to the end of its latest compaction; `None`
/// when nothing was compacted.
///
/// Nothing after a compaction's end refers to a request or call before it: a compaction ends where
/// a Round ended, requests and calls start and settle within their Round, and recovery settles an
/// interrupted Round before recording its end.
pub(crate) fn project_reach(conversation: &[&Entry]) -> Option<LogPos> {
    compaction(conversation).map(|(upto, _)| upto)
}

/// How far back `entries` must be read to project the transcript of their conversation.
pub(crate) fn transcript_reach(entries: &[Entry]) -> Option<LogPos> {
    let conversation = conversation(entries);
    conversation_reach(entries, &conversation, project_reach(&conversation))
}

/// The latest compaction in a conversation: where it ended, and its summary.
fn compaction<'a>(conversation: &[&'a Entry]) -> Option<(LogPos, &'a String)> {
    conversation
        .iter()
        .rev()
        .find_map(|entry| match &entry.body {
            EntryBody::Compacted { upto, summary, .. } => Some((*upto, summary)),
            _ => None,
        })
}

pub(crate) fn project(entries: &[&Entry]) -> Transcript {
    let compacted = compaction(entries);
    let mut transcript = Transcript {
        summary: compacted.map(|(_, summary)| summary.clone()),
        ..Default::default()
    };
    let mut attempts = HashMap::new();
    let mut calls = HashMap::new();
    let visible = entries
        .iter()
        .filter(|entry| compacted.is_none_or(|(upto, _)| entry.pos > upto));
    for entry in visible {
        match &entry.body {
            EntryBody::AttemptStarted {
                attempt,
                purpose,
                provider,
                ..
            } => {
                attempts.insert(*attempt, (*purpose, provider));
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
        let (purpose, provider, call) = match &entry.body {
            EntryBody::AttemptSettled { attempt, .. } => {
                let info = attempts.get(attempt);
                (
                    info.map(|(purpose, _)| *purpose),
                    info.map(|(_, provider)| *(*provider)),
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
                provider,
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
