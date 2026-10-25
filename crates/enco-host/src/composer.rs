mod compaction;
mod images;
mod recovery;

use crate::limits::*;
use chrono::{Offset, Timelike};
use compaction::Compaction;
use enco_core::*;
use enco_kernel::*;
use std::path::PathBuf;

// Budgeting uses a conservative estimate; the Provider reports actual usage.
fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.len().div_ceil(3)).unwrap_or(u32::MAX)
}

/// Factory policy shared by ordinary operation and safe mode. No IO occurs here.
pub struct FactoryComposer {
    home: PathBuf,
    workspace: PathBuf,
    instructions: PathBuf,
}

impl FactoryComposer {
    pub fn new(home: PathBuf, workspace: PathBuf, instructions: PathBuf) -> Self {
        Self {
            home,
            workspace,
            instructions,
        }
    }
}

impl Composer for FactoryComposer {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "factory-composer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError> {
        let budget = input.reply.budget;
        let mut omitted = input.context.omitted.clone();
        let mut memory_budget = budget.context_tokens.get().saturating_mul(MEMORY_PERCENT) / 100;
        let pinned = select_memories(
            input
                .context
                .candidates
                .iter()
                .filter(|c| c.kind == CandidateKind::Memory && c.standing),
            &mut memory_budget,
            &mut omitted,
        );
        let (system, sources) = self.system(input, budget, &pinned, &mut omitted);
        let system_tokens = estimate_tokens(&system);
        let history = annotated_history(input);
        let selected_images = images::selected(input, &mut omitted);
        let items = reply_items(
            input,
            PlanItem::Message {
                message: Message::text(Role::System, system),
                sources,
            },
            &history,
            &selected_images,
            &mut memory_budget,
            &mut omitted,
        );
        let sizes = history_sizes(&items[1..], &input.transcript)?;
        let tools: Vec<_> = input
            .tools
            .iter()
            .map(|(id, spec)| (id.clone(), DisclosedTool::from(spec)))
            .collect();
        let definitions: Vec<_> = tools.iter().map(|(_, tool)| tool).collect();
        let history_tokens: u32 = sizes.iter().map(|(_, size)| size).sum();
        let total = system_tokens
            .saturating_add(history_tokens)
            .saturating_add(encoded_tokens(&definitions)?)
            .saturating_add(budget.max_output_tokens.get());
        let window = budget.context_tokens.get();
        if total > window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100 {
            if input.compactions_left > 0
                && let Some(budget) = input.compaction
            {
                let keep = window.saturating_mul(COMPACTION_TAIL_PERCENT) / 100;
                let compaction = Compaction::new(input, &history, budget);
                if let Some(upto) = compaction.boundary(input, &sizes, history_tokens, keep) {
                    return Ok(Composition::Compact {
                        upto,
                        plan: compaction.plan(upto),
                    });
                }
            }
            if total > window {
                return Err(match input.compaction {
                    None => ComposeError::CompactionUnavailable,
                    Some(_) => ComposeError::ContextOverflow {
                        needed: total,
                        window,
                    },
                });
            }
        }
        Ok(Composition::Plan(ContextPlan {
            items,
            tools,
            max_output_tokens: Some(budget.max_output_tokens.get()),
            omitted,
        }))
    }
}

impl FactoryComposer {
    fn system(
        &self,
        input: &ComposeInput,
        budget: Budget,
        pinned: &[&Candidate],
        omitted: &mut Vec<Omission>,
    ) -> (String, Vec<Source>) {
        let mut text = if input.safe_mode.is_some() {
            include_str!("prompts/safe_mode.md")
        } else {
            include_str!("prompts/system.md")
        }
        .trim()
        .to_string();
        text.push_str(&format!(
            "\n\n## Environment\n- Home: {} (config.toml and plugins/)\n- Manual: {} (start with README.md; generated, so edits are lost)\n- Workspace: {}\n- Standing instructions: {}\n- Session: {}\n- Time zone: {} (UTC{})",
            self.home.display(), self.home.join(crate::development::MANUAL_DIRECTORY).display(), self.workspace.display(), self.instructions.display(), input.session.name,
            input.timezone, input.now.with_timezone(&input.timezone).format("%:z")
        ));
        let mut sources = Vec::new();
        recovery::append(&mut text, input);
        if !input.reply.accepts.contains(&Modality::Image)
            && input.transcript.items.iter().any(|item| {
                item.message
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Image(_)))
            })
        {
            text.push_str("\n\nThis reply model cannot view images. Their descriptions include file paths; use tools to work with those files when needed.");
        }
        append_instructions(
            &mut text,
            &mut sources,
            omitted,
            input,
            budget.context_tokens.get(),
        );
        append_memories(&mut text, &mut sources, pinned, "## Your pinned memories");
        if let Some(summary) = &input.transcript.summary {
            section(
                &mut text,
                "Your summary of the earlier conversation",
                summary,
            );
        }
        (text, sources)
    }
}

/// Keep the recorded presentation while its head and visible history still apply.
fn reply_items(
    input: &ComposeInput,
    head: PlanItem,
    history: &[(&TranscriptItem, String)],
    selected_images: &std::collections::BTreeSet<LogPos>,
    memory_budget: &mut u32,
    omitted: &mut Vec<Omission>,
) -> Vec<PlanItem> {
    let previous = input.previous_plan.as_ref().filter(|plan| {
        plan.items.first() == Some(&head)
            && plan.items.iter().all(|item| match item {
                PlanItem::Log { pos, images } => {
                    *images == selected_images.contains(pos)
                        && input
                            .transcript
                            .items
                            .binary_search_by_key(pos, |item| item.pos)
                            .is_ok()
                }
                PlanItem::Message { .. } => true,
            })
    });
    let mut items = previous.map_or_else(|| vec![head], |plan| plan.items.clone());
    let after = items.iter().rev().find_map(|item| match item {
        PlanItem::Log { pos, .. } => Some(*pos),
        _ => None,
    });
    let latest_input = input
        .transcript
        .items
        .iter()
        .rev()
        .find(|item| item.event.is_some())
        .map(|item| item.pos);
    for (item, annotation) in history
        .iter()
        .filter(|(item, _)| after.is_none_or(|pos| item.pos > pos))
    {
        let mut note = annotation.clone();
        let mut sources = Vec::new();
        if Some(item.pos) == latest_input {
            let recalled = select_memories(
                input
                    .context
                    .candidates
                    .iter()
                    .filter(|c| c.kind == CandidateKind::Memory && !c.standing)
                    .filter(|c| {
                        last_source(&items, &c.id) != Some(ContentHash::of(c.text.as_bytes()))
                    }),
                memory_budget,
                omitted,
            );
            append_memories(
                &mut note,
                &mut sources,
                &recalled,
                "Memories you recall for the next message:",
            );
        }
        // A note describes the agent's own circumstances, yet it takes the user role: some
        // protocols accept system messages only at the start, and some chat templates move
        // every system message there.
        if !note.is_empty() {
            items.push(PlanItem::Message {
                message: Message::text(Role::User, note),
                sources,
            });
        }
        items.push(PlanItem::Log {
            pos: item.pos,
            images: selected_images.contains(&item.pos),
        });
    }
    items
}

fn last_source(items: &[PlanItem], id: &str) -> Option<ContentHash> {
    items
        .iter()
        .rev()
        .filter_map(|item| match item {
            PlanItem::Message { sources, .. } => Some(sources),
            PlanItem::Log { .. } => None,
        })
        .flat_map(|sources| sources.iter().rev())
        .find(|source| source.id == id)
        .map(|source| source.hash)
}

/// Each inline note belongs to the Log item immediately after it, including when reused.
fn history_sizes(
    items: &[PlanItem],
    transcript: &Transcript,
) -> Result<Vec<(LogPos, u32)>, ComposeError> {
    let mut sizes = Vec::new();
    let mut pending = 0u32;
    for item in items {
        match item {
            PlanItem::Message { message, .. } => {
                pending = pending.saturating_add(encoded_tokens(message)?)
            }
            PlanItem::Log { pos, images } => {
                let index = transcript
                    .items
                    .binary_search_by_key(pos, |item| item.pos)
                    .map_err(|_| {
                        ComposeError::Invalid(format!("Log position {pos:?} is not visible"))
                    })?;
                let size = pending.saturating_add(images::message_tokens(
                    &transcript.items[index].message,
                    *images,
                )?);
                sizes.push((*pos, size));
                pending = 0;
            }
        }
    }
    Ok(sizes)
}

use crate::clock::LOCAL_TIME;

/// Describe the recorded circumstances of each input once for both reply and compaction.
///
/// Arrival times appear only when the local hour changes, to the minute and without an
/// offset: the time zone under Environment is their reference, so a note depends only on
/// recorded facts and the cached prefix stays stable. The request carries no current time;
/// the agent asks the shell when it needs one.
fn annotated_history(input: &ComposeInput) -> Vec<(&TranscriptItem, String)> {
    let mut previous_hour = None;
    let mut run_ends = input.transcript.run_ends.iter().peekable();
    input
        .transcript
        .items
        .iter()
        .map(|item| {
            let mut note = String::new();
            if let Some(event) = &item.event {
                let local = event.received_at.with_timezone(&input.timezone);
                // The offset separates the hour repeated when daylight saving time ends.
                let hour = Some((local.date_naive(), local.hour(), local.offset().fix()));
                if hour != previous_hour {
                    note = format!("[{}]", local.format(LOCAL_TIME));
                }
                previous_hour = hour;
                // Consume outcomes only at inputs, so each belongs to the first input after it.
                let mut previous_end = None;
                while let Some((_, end)) = run_ends.next_if(|(pos, _)| *pos < item.pos) {
                    previous_end = Some(end);
                }
                for part in [
                    previous_end.and_then(work_stopped),
                    producer(&event.body, input.timezone),
                    images::attachments(input, &event.body),
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
            (item, note)
        })
        .collect()
}

fn work_stopped(end: &RunEnd) -> Option<String> {
    let reason = match end {
        RunEnd::Completed => return None,
        RunEnd::Interrupted => "was interrupted".into(),
        RunEnd::Cancelled => "was cancelled".into(),
        RunEnd::BudgetExhausted => "reached the step limit".into(),
        RunEnd::Failed { failure } => format!("failed ({}: {})", failure.code, failure.message),
    };
    Some(format!(
        "Your previous work {reason} before it finished. The tool results above show what you did and which actions remain uncertain."
    ))
}

/// Name the producer of an input that did not come from the owner.
fn producer(body: &EventBody, timezone: Tz) -> Option<String> {
    match body {
        EventBody::UserMessage { .. } => None,
        EventBody::Reminder {
            due_at, skipped, ..
        } => {
            let mut text = format!(
                "The next message is a reminder you scheduled for {}.",
                due_at.with_timezone(&timezone).format(LOCAL_TIME)
            );
            if *skipped > 0 {
                text.push_str(&format!(" Missed earlier occurrences: {skipped}."));
            }
            Some(text)
        }
        EventBody::GenerationRolledBack { .. } => {
            Some("The next message is a notice from the plugin registry.".into())
        }
    }
}

fn source(candidate: &Candidate) -> Source {
    Source {
        id: candidate.id.clone(),
        hash: ContentHash::of(candidate.text.as_bytes()),
    }
}

fn append_instructions(
    text: &mut String,
    sources: &mut Vec<Source>,
    omitted: &mut Vec<Omission>,
    input: &ComposeInput,
    window: u32,
) {
    let instructions: Vec<_> = input
        .context
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Instruction && c.standing)
        .collect();
    let size: u32 = instructions.iter().map(|c| estimate_tokens(&c.text)).sum();
    let budget = window.saturating_mul(INSTRUCTION_PERCENT) / 100;
    if size <= budget {
        let body = instructions
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        section(text, "Standing instructions (AGENTS.md)", &body);
        sources.extend(instructions.into_iter().map(source));
    } else {
        omitted.extend(instructions.iter().map(|c| Omission {
            source: c.id.clone(),
            reason: format!("instructions exceed {INSTRUCTION_PERCENT}% of the context window"),
        }));
    }
}

fn select_memories<'a>(
    candidates: impl Iterator<Item = &'a Candidate>,
    remaining: &mut u32,
    omitted: &mut Vec<Omission>,
) -> Vec<&'a Candidate> {
    let mut selected = Vec::new();
    for candidate in candidates {
        let size = estimate_tokens(&candidate.text);
        if size <= *remaining {
            *remaining -= size;
            selected.push(candidate);
        } else {
            omitted.push(Omission {
                source: candidate.id.clone(),
                reason: format!("memory budget ({MEMORY_PERCENT}% of the context window) exceeded"),
            });
        }
    }
    selected
}

fn append_memories(
    text: &mut String,
    sources: &mut Vec<Source>,
    memories: &[&Candidate],
    heading: &str,
) {
    if memories.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(heading);
    for candidate in memories {
        text.push_str(&format!(
            "\n- {} (id: {})",
            candidate.text,
            candidate
                .id
                .strip_prefix("memory:")
                .unwrap_or(&candidate.id)
        ));
        sources.push(source(candidate));
    }
}

fn section(text: &mut String, title: &str, body: &str) {
    if !body.is_empty() {
        text.push_str(&format!("\n\n## {title}\n{body}"));
    }
}

fn encoded_tokens(value: &impl serde::Serialize) -> Result<u32, ComposeError> {
    serde_json::to_string(value)
        .map(|text| estimate_tokens(&text))
        .map_err(|e| ComposeError::Invalid(e.to_string()))
}
