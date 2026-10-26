//! Keeps recorded notes stable and annotates inputs with arrival time, source and previous work outcomes.
use crate::composer::*;
use crate::{context, images, key, limits::*, tokens};
use chrono::{Offset, Timelike, Utc};
use chrono_tz::Tz;
use enco_sdk::types::{Failure, Role};
use std::collections::BTreeSet;

const LOCAL_TIME: &str = "%Y-%m-%d %H:%M, %A";

/// Keep the recorded presentation while its head and visible history still apply, appending only
/// what is new, so the request shares its cached prefix with the previous one.
pub(super) fn reply_items(
    input: &ComposeInput,
    head: PlanItem,
    history: &[(&TranscriptItem, String)],
    selected_images: &BTreeSet<(u64, u64)>,
    memory_budget: &mut u32,
    omitted: &mut Vec<Omission>,
) -> Vec<PlanItem> {
    let previous = input.previous.as_ref().filter(|plan| {
        plan.items.first() == Some(&head)
            && plan.items.iter().all(|item| match item {
                PlanItem::Log(reference) => {
                    reference.images == selected_images.contains(&key(&reference.pos))
                        && input
                            .transcript
                            .items
                            .binary_search_by_key(&key(&reference.pos), |item| key(&item.pos))
                            .is_ok()
                }
                PlanItem::Message(_) => true,
            })
    });
    let mut items = previous.map_or_else(|| vec![head], |plan| plan.items.clone());
    let after = items.iter().rev().find_map(|item| match item {
        PlanItem::Log(reference) => Some(key(&reference.pos)),
        _ => None,
    });
    let latest_input = input
        .transcript
        .items
        .iter()
        .rev()
        .find(|item| item.event.is_some())
        .map(|item| key(&item.pos));
    for (item, annotation) in history
        .iter()
        .filter(|(item, _)| after.is_none_or(|pos| key(&item.pos) > pos))
    {
        let mut note = annotation.clone();
        let mut sources = vec![];
        if Some(key(&item.pos)) == latest_input {
            let recalled = context::select_memories(
                input
                    .memories
                    .iter()
                    .filter(|memory| !memory.pinned)
                    .filter(|memory| {
                        let source = context::memory_source(memory);
                        last_source(&items, &source.id) != Some(source.hash.as_str())
                    }),
                memory_budget,
                omitted,
            );
            context::append_memories(
                &mut note,
                &mut sources,
                &recalled,
                "Memories you recall for the next message:",
            );
        }
        // Some protocols accept system messages only at the start, so circumstance notes use the user role.
        if !note.is_empty() {
            items.push(PlanItem::Message(Inline {
                role: Role::User,
                text: note,
                sources,
            }));
        }
        items.push(PlanItem::Log(LogRef {
            pos: item.pos,
            images: selected_images.contains(&key(&item.pos)),
        }));
    }
    items
}

fn last_source<'a>(items: &'a [PlanItem], id: &str) -> Option<&'a str> {
    items
        .iter()
        .rev()
        .filter_map(|item| match item {
            PlanItem::Message(message) => Some(&message.sources),
            PlanItem::Log(_) => None,
        })
        .flat_map(|sources| sources.iter().rev())
        .find(|source| source.id == id)
        .map(|source| source.hash.as_str())
}

/// Each inline note belongs to the Log item immediately after it, including when reused.
pub(super) fn sizes(
    items: &[PlanItem],
    transcript: &Transcript,
) -> Result<Vec<(LogPos, u32)>, Failure> {
    let mut sizes = vec![];
    let mut pending = 0u32;
    for item in items {
        match item {
            PlanItem::Message(message) => {
                pending = pending.saturating_add(tokens(
                    message.text.len() + MESSAGE_OVERHEAD_BYTES + PART_OVERHEAD_BYTES,
                ))
            }
            PlanItem::Log(reference) => {
                let index = transcript
                    .items
                    .binary_search_by_key(&key(&reference.pos), |item| key(&item.pos))
                    .map_err(|_| {
                        crate::failed(format!("Log position {:?} is not visible", reference.pos))
                    })?;
                let size = pending.saturating_add(images::message_tokens(
                    &transcript.items[index].message,
                    reference.images,
                ));
                sizes.push((reference.pos, size));
                pending = 0;
            }
        }
    }
    Ok(sizes)
}

pub(super) fn instant(datetime: &Datetime) -> Result<chrono::DateTime<Utc>, Failure> {
    let seconds = i64::try_from(datetime.seconds)
        .map_err(|error| crate::failed(format!("timestamp seconds: {error}")))?;
    chrono::DateTime::from_timestamp(seconds, datetime.nanoseconds).ok_or_else(|| {
        crate::failed(format!(
            "timestamp {seconds}.{} is outside the supported range",
            datetime.nanoseconds
        ))
    })
}

/// Describe the recorded circumstances of each input once, for replies and compaction alike.
///
/// Arrival times appear only when the local hour changes, to the minute and without an offset: the
/// time zone under Environment is their reference, so a note depends only on recorded facts and the
/// cached prefix stays stable. The request carries no current time; the agent asks the shell.
pub(super) fn annotated(
    input: &ComposeInput,
    timezone: Tz,
) -> Result<Vec<(&TranscriptItem, String)>, Failure> {
    let mut previous_hour = None;
    let mut run_ends = input.transcript.run_ends.iter().peekable();
    input
        .transcript
        .items
        .iter()
        .map(|item| {
            let mut note = String::new();
            if let Some(event) = &item.event {
                let local = instant(&event.received_at)?.with_timezone(&timezone);
                // The offset separates the hour repeated when daylight saving time ends.
                let hour = Some((local.date_naive(), local.hour(), local.offset().fix()));
                if hour != previous_hour {
                    note = format!("[{}]", local.format(LOCAL_TIME));
                }
                previous_hour = hour;
                // Consume outcomes only at inputs, so each belongs to the first input after it.
                let mut previous_end = None;
                while let Some(ended) = run_ends.next_if(|ended| key(&ended.pos) < key(&item.pos)) {
                    previous_end = Some(&ended.end);
                }
                for part in [
                    previous_end.and_then(work_stopped),
                    producer(&event.body, timezone)?,
                    images::attachments(&event.body),
                ]
                .into_iter()
                .flatten()
                {
                    if !note.is_empty() {
                        note.push_str("\n\n");
                    }
                    note.push_str(&part);
                }
            }
            Ok((item, note))
        })
        .collect()
}

fn work_stopped(end: &RunEnd) -> Option<String> {
    let reason = match end {
        RunEnd::Completed => return None,
        RunEnd::Interrupted => "was interrupted".into(),
        RunEnd::Cancelled => "was cancelled".into(),
        RunEnd::BudgetExhausted => "reached the step limit".into(),
        RunEnd::Failed(failure) => format!("failed ({}: {})", failure.code, failure.message),
    };
    Some(format!(
        "Your previous work {reason} before it finished. The tool results above show what you did and which actions remain uncertain."
    ))
}

fn producer(body: &EventBody, timezone: Tz) -> Result<Option<String>, Failure> {
    Ok(match body {
        EventBody::UserMessage(_) => None,
        EventBody::Reminder(reminder) => {
            let mut text = format!(
                "The next message is a reminder you scheduled for {}.",
                instant(&reminder.due_at)?
                    .with_timezone(&timezone)
                    .format(LOCAL_TIME)
            );
            if reminder.skipped > 0 {
                text.push_str(&format!(
                    " Missed earlier occurrences: {}.",
                    reminder.skipped
                ));
            }
            Some(text)
        }
        EventBody::GenerationRolledBack(_) => {
            Some("The next message is a notice from the plugin registry.".into())
        }
    })
}
