use crate::{Transcript, TranscriptItem};
use enco_core::*;
use std::collections::HashMap;

pub(crate) fn project(entries: &[Entry]) -> Transcript {
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
