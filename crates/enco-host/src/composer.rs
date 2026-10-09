use crate::limits::*;
use chrono::{Offset, Timelike};
use enco_core::*;
use enco_kernel::*;
use std::{collections::HashMap, path::PathBuf};

// Budgeting uses a conservative estimate; the Provider reports actual usage.
fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.len().div_ceil(3)).unwrap_or(u32::MAX)
}

/// Factory policy shared by ordinary operation and safe mode. No IO occurs here.
pub struct FactoryComposer {
    workspace: PathBuf,
    instructions: PathBuf,
}

impl FactoryComposer {
    pub fn new(workspace: PathBuf, instructions: PathBuf) -> Self {
        Self {
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
        let budget = input.profile.reply.budget;
        let mut omitted = input.context.omitted.clone();
        let mut memory_budget = budget.context_tokens.saturating_mul(MEMORY_PERCENT) / 100;
        let pinned = select_memories(
            input
                .context
                .candidates
                .iter()
                .filter(|c| c.kind == CandidateKind::Memory && c.standing),
            &mut memory_budget,
            &mut omitted,
        );
        let (system, sources) = self.system(input, &pinned, &mut omitted);
        let system_tokens = estimate_tokens(&system);
        let history = annotated_history(input);
        let items = reply_items(
            input,
            PlanItem::Message {
                message: Message::text(Role::System, system),
                sources,
            },
            &history,
            &mut memory_budget,
            &mut omitted,
        );
        let sizes = history_sizes(&items[1..], &input.transcript)?;
        let definitions: Vec<_> = input.tools.iter().map(|(_, spec)| spec).collect();
        let history_tokens: u32 = sizes.iter().map(|(_, size)| size).sum();
        let total = system_tokens
            .saturating_add(history_tokens)
            .saturating_add(encoded_tokens(&definitions)?)
            .saturating_add(budget.max_output_tokens);
        let window = budget.context_tokens;
        if total > window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100 {
            if input.compactions_left > 0 {
                let keep = window.saturating_mul(COMPACTION_TAIL_PERCENT) / 100;
                let compaction = Compaction::new(input, &history);
                if let Some(upto) = compaction.boundary(input, &sizes, history_tokens, keep) {
                    return Ok(Composition::Compact {
                        upto,
                        plan: compaction.plan(upto),
                    });
                }
            }
            if total > window {
                return Err(ComposeError::ContextOverflow {
                    needed: total,
                    window,
                });
            }
        }
        Ok(Composition::Plan(ContextPlan {
            items,
            tools: input.tools.clone(),
            max_output_tokens: Some(budget.max_output_tokens),
            omitted,
        }))
    }
}

impl FactoryComposer {
    fn system(
        &self,
        input: &ComposeInput,
        pinned: &[&Candidate],
        omitted: &mut Vec<Omission>,
    ) -> (String, Vec<Source>) {
        let mut text = if input.safe_mode {
            include_str!("prompts/safe_mode.md")
        } else {
            include_str!("prompts/system.md")
        }
        .trim()
        .to_string();
        text.push_str(&format!(
            "\n\n## Environment\n- Workspace: {}\n- Standing instructions: {}\n- Session: {}\n- Time zone: {} (UTC{})",
            self.workspace.display(), self.instructions.display(), input.session.name,
            input.timezone, input.now.with_timezone(&input.timezone).format("%:z")
        ));
        let mut sources = Vec::new();
        append_instructions(&mut text, &mut sources, omitted, input);
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
    memory_budget: &mut u32,
    omitted: &mut Vec<Omission>,
) -> Vec<PlanItem> {
    let previous = input.previous_plan.as_ref().filter(|plan| {
        plan.items.first() == Some(&head)
            && plan.items.iter().all(|item| match item {
                PlanItem::Log { pos } => input
                    .transcript
                    .items
                    .binary_search_by_key(pos, |item| item.pos)
                    .is_ok(),
                PlanItem::Message { .. } => true,
            })
    });
    let mut items = previous.map_or_else(|| vec![head], |plan| plan.items.clone());
    let after = items.iter().rev().find_map(|item| match item {
        PlanItem::Log { pos } => Some(*pos),
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
        if !note.is_empty() {
            items.push(PlanItem::Message {
                message: Message::text(Role::User, note),
                sources,
            });
        }
        items.push(PlanItem::Log { pos: item.pos });
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
            PlanItem::Log { pos } => {
                let index = transcript
                    .items
                    .binary_search_by_key(pos, |item| item.pos)
                    .map_err(|_| {
                        ComposeError::Invalid(format!("Log position {pos:?} is not visible"))
                    })?;
                let size =
                    pending.saturating_add(encoded_tokens(&transcript.items[index].message)?);
                sizes.push((*pos, size));
                pending = 0;
            }
        }
    }
    Ok(sizes)
}

const LOCAL_TIME: &str = "%Y-%m-%d %H:%M, %A";

/// Describe the recorded circumstances of each input once for both reply and compaction.
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
) {
    let instructions: Vec<_> = input
        .context
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Instruction && c.standing)
        .collect();
    let size: u32 = instructions.iter().map(|c| estimate_tokens(&c.text)).sum();
    let budget = input
        .profile
        .reply
        .budget
        .context_tokens
        .saturating_mul(INSTRUCTION_PERCENT)
        / 100;
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

const COMPACTION_PROMPT: &str = include_str!("prompts/compaction.md");
const COMPACTION_FOOTER: &str = "\nWrite the summary now.";

/// The history as the summarizer reads it. Choosing the boundary and building the plan share
/// one rendering, so the plan fits the window the boundary was chosen for.
struct Compaction {
    header: String,
    /// One rendered chunk per Transcript item, in Log order.
    chunks: Vec<(LogPos, String)>,
    max_output_tokens: u32,
    /// Tokens the request needs besides the chunks: prompt, header, footer and output.
    overhead: u32,
}

impl Compaction {
    fn new(input: &ComposeInput, history: &[(&TranscriptItem, String)]) -> Self {
        let mut header = String::new();
        if let Some(summary) = &input.transcript.summary {
            header.push_str(&format!("Previous summary:\n{summary}\n\n"));
        }
        header.push_str("Conversation to summarize:\n");
        let mut names = HashMap::new();
        let chunks = history
            .iter()
            .map(|(item, annotation)| {
                let mut chunk = String::new();
                if !annotation.is_empty() {
                    chunk.push_str(annotation);
                    chunk.push('\n');
                }
                for part in &item.message.parts {
                    match part {
                        Part::Text { text } => {
                            let speaker = match item.event.as_ref().map(|event| &event.body) {
                                Some(EventBody::UserMessage { .. }) => "Owner",
                                Some(EventBody::Reminder { .. }) => "Reminder",
                                Some(EventBody::GenerationRolledBack { .. }) => "Notice",
                                None => "You",
                            };
                            chunk.push_str(&format!("{speaker}: {text}\n"));
                        }
                        Part::ToolCall(call) => {
                            names.insert(call.id, call.name.as_str());
                            let arguments = clip(&call.arguments);
                            chunk.push_str(&format!("You called {} with {arguments}\n", call.name));
                        }
                        Part::ToolResult(result) => {
                            let name = names.get(&result.call).copied().unwrap_or("tool");
                            chunk.push_str(&format!(
                                "Result of {name}: {}\n",
                                clip(&result.content)
                            ));
                        }
                        Part::Extension(_) => {}
                    }
                }
                (item.pos, chunk)
            })
            .collect();
        let max_output_tokens = input
            .profile
            .compaction
            .budget
            .max_output_tokens
            .min(COMPACTION_OUTPUT_TOKENS);
        // Estimating parts separately never undercounts their concatenation.
        let overhead = estimate_tokens(COMPACTION_PROMPT)
            .saturating_add(estimate_tokens(&header))
            .saturating_add(estimate_tokens(COMPACTION_FOOTER))
            .saturating_add(max_output_tokens);
        Self {
            header,
            chunks,
            max_output_tokens,
            overhead,
        }
    }

    /// The earliest Round end that leaves at most `keep` tokens of history, or, when
    /// summarizing that far would not fit the compaction window, the latest Round end that
    /// does. Each summary carries the previous one forward, so a partial compaction is progress.
    fn boundary(
        &self,
        input: &ComposeInput,
        sizes: &[(LogPos, u32)],
        history_tokens: u32,
        keep: u32,
    ) -> Option<LogPos> {
        let window = input.profile.compaction.budget.context_tokens;
        let mut tail = history_tokens;
        let mut needed = self.overhead;
        let mut sizes = sizes.iter().peekable();
        let mut chunks = self.chunks.iter().peekable();
        let mut fitting = None;
        // All three sequences follow Log order, so each item is counted once.
        for &end in &input.transcript.round_ends {
            while let Some((_, size)) = sizes.next_if(|(pos, _)| *pos <= end) {
                tail -= size;
            }
            while let Some((_, chunk)) = chunks.next_if(|(pos, _)| *pos <= end) {
                needed = needed.saturating_add(estimate_tokens(chunk));
            }
            if needed > window {
                break;
            }
            fitting = Some(end);
            if tail <= keep {
                break;
            }
        }
        fitting
    }

    fn plan(&self, upto: LogPos) -> ContextPlan {
        let mut text = self.header.clone();
        for (_, chunk) in self.chunks.iter().take_while(|(pos, _)| *pos <= upto) {
            text.push_str(chunk);
        }
        text.push_str(COMPACTION_FOOTER);
        ContextPlan {
            items: vec![
                PlanItem::Message {
                    message: Message::text(Role::System, COMPACTION_PROMPT),
                    sources: vec![],
                },
                PlanItem::Message {
                    message: Message::text(Role::User, text),
                    sources: vec![],
                },
            ],
            tools: vec![],
            max_output_tokens: Some(self.max_output_tokens),
            omitted: vec![],
        }
    }
}

/// Tool arguments and results are evidence for the summary, not its subject; long ones are cut.
fn clip(text: &str) -> &str {
    &text[..text.floor_char_boundary(COMPACTION_TOOL_BYTES)]
}

fn encoded_tokens(value: &impl serde::Serialize) -> Result<u32, ComposeError> {
    serde_json::to_string(value)
        .map(|text| estimate_tokens(&text))
        .map_err(|e| ComposeError::Invalid(e.to_string()))
}
