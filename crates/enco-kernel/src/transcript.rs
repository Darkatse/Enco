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
    let mut purposes = HashMap::new();
    let mut calls = HashMap::new();
    // Resolve call identities even before the compaction boundary; only visibility is filtered.
    for entry in entries {
        match &entry.body {
            EntryBody::AttemptStarted {
                attempt, purpose, ..
            } => {
                purposes.insert(*attempt, *purpose);
            }
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { message, .. },
            } if purposes.get(attempt) == Some(&AttemptPurpose::Reply) => {
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
        let (purpose, call) = match &entry.body {
            EntryBody::AttemptSettled { attempt, .. } => (purposes.get(attempt).copied(), None),
            EntryBody::ToolCallSettled { call, .. } => (None, calls.get(call).copied()),
            _ => (None, None),
        };
        if let Some(message) = entry.body.canonical_message(purpose, call) {
            transcript.items.push(TranscriptItem {
                pos: entry.pos,
                message,
            });
        }
        if matches!(entry.body, EntryBody::RoundEnded { .. }) {
            transcript.round_ends.push(entry.pos);
        }
    }
    transcript
}
