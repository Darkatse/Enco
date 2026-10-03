use crate::limits::*;
use chrono::Timelike;
use enco_core::*;
use enco_kernel::*;
use std::{collections::HashMap, path::PathBuf};

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
        let system = self.system(input, &mut omitted);
        let context = current_context(input, &mut omitted);
        let history = timed_history(input);
        let sizes = history
            .iter()
            .map(|(item, marker)| {
                encoded_tokens(&item.message).map(|size| {
                    (
                        item.pos,
                        size.saturating_add(marker.as_deref().map_or(0, estimate_tokens)),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let definitions: Vec<_> = input.tools.iter().map(|(_, spec)| spec).collect();
        let history_tokens: u32 = sizes.iter().map(|(_, size)| size).sum();
        let total = estimate_tokens(&system)
            .saturating_add(history_tokens)
            .saturating_add(estimate_tokens(&context))
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
        let mut items = vec![PlanItem::Message {
            message: Message::text(Role::System, system),
        }];
        for (item, marker) in history {
            if let Some(marker) = marker {
                items.push(PlanItem::Message {
                    message: Message::text(Role::User, marker),
                });
            }
            items.push(PlanItem::Log { pos: item.pos });
        }
        items.push(PlanItem::Message {
            message: Message::text(Role::User, context),
        });
        Ok(Composition::Plan(ContextPlan {
            items,
            tools: input.tools.clone(),
            max_output_tokens: Some(budget.max_output_tokens),
            omitted,
        }))
    }
}

impl FactoryComposer {
    fn system(&self, input: &ComposeInput, omitted: &mut Vec<Omission>) -> String {
        let mut text = if input.safe_mode {
            include_str!("prompts/safe_mode.md")
        } else {
            include_str!("prompts/system.md")
        }
        .trim()
        .to_string();
        text.push_str(&format!(
            "\n\n## Environment\n- Workspace: {}\n- Standing instructions: {}\n- Session: {}",
            self.workspace.display(),
            self.instructions.display(),
            input.session.name
        ));
        append_instructions(&mut text, omitted, input);
        if let Some(summary) = &input.transcript.summary {
            section(
                &mut text,
                "Your summary of the earlier conversation",
                summary,
            );
        }
        text
    }
}

/// The one way the Agent reads a time: the current time and the arrival markers share it.
const LOCAL_TIME: &str = "%Y-%m-%d %H:%M, %A";

fn current_context(input: &ComposeInput, omitted: &mut Vec<Omission>) -> String {
    let mut text = format!(
        "[context]\nCurrent time: {} (UTC{})",
        input.now.format(LOCAL_TIME),
        input.now.format("%:z")
    );
    append_memories(&mut text, omitted, input);
    let ending = match &input.previous_run_end {
        Some(RunEnd::Interrupted) => "was interrupted before it finished.".into(),
        Some(RunEnd::Cancelled) => "was cancelled before it finished.".into(),
        Some(RunEnd::BudgetExhausted) => "reached the step limit before it finished.".into(),
        Some(RunEnd::Failed { failure }) => format!("stopped after an error: {}", failure.message),
        None | Some(RunEnd::Completed) => return text,
    };
    text.push_str(&format!(
        "\n\nYour previous work {ending}\nThe tool results above show what you did and which actions remain uncertain."
    ));
    text
}

/// Marks the first Inbox input and each one arriving in a new local clock hour. Rendered once
/// and shared by reply assembly, budgeting and compaction.
fn timed_history(input: &ComposeInput) -> Vec<(&TranscriptItem, Option<String>)> {
    let mut previous_hour = None;
    input
        .transcript
        .items
        .iter()
        .map(|item| {
            let marker = item.received_at.and_then(|at| {
                let local = at.with_timezone(input.now.offset());
                let hour = Some((local.date_naive(), local.hour()));
                let changed = hour != previous_hour;
                previous_hour = hour;
                changed.then(|| format!("[{}]", local.format(LOCAL_TIME)))
            });
            (item, marker)
        })
        .collect()
}

fn append_instructions(text: &mut String, omitted: &mut Vec<Omission>, input: &ComposeInput) {
    let instructions: Vec<_> = input
        .context
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Instruction)
        .collect();
    let size = instructions
        .iter()
        .map(|c| estimate_tokens(&c.text))
        .sum::<u32>();
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
    } else {
        omitted.extend(instructions.iter().map(|c| Omission {
            source: c.id.clone(),
            reason: format!("instructions exceed {INSTRUCTION_PERCENT}% of the context window"),
        }));
    }
}

fn append_memories(text: &mut String, omitted: &mut Vec<Omission>, input: &ComposeInput) {
    let budget = input
        .profile
        .reply
        .budget
        .context_tokens
        .saturating_mul(MEMORY_PERCENT)
        / 100;
    let mut memory = String::new();
    let mut used = 0;
    for candidate in input
        .context
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Memory)
    {
        let size = estimate_tokens(&candidate.text);
        if used + size <= budget {
            used += size;
            memory.push_str(&format!(
                "- {} (id: {})\n",
                candidate.text,
                candidate
                    .id
                    .strip_prefix("memory:")
                    .unwrap_or(&candidate.id)
            ));
        } else {
            omitted.push(Omission {
                source: candidate.id.clone(),
                reason: format!("memory budget ({MEMORY_PERCENT}% of the context window) exceeded"),
            });
        }
    }
    if !memory.is_empty() {
        text.push_str("\n\nYour memories (authoritative; they override anything said earlier in the conversation):\n");
        text.push_str(memory.trim_end());
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
    fn new(input: &ComposeInput, history: &[(&TranscriptItem, Option<String>)]) -> Self {
        let mut header = String::new();
        if let Some(summary) = &input.transcript.summary {
            header.push_str(&format!("Previous summary:\n{summary}\n\n"));
        }
        header.push_str("Conversation to summarize:\n");
        let mut names = HashMap::new();
        let chunks = history
            .iter()
            .map(|(item, marker)| {
                let mut chunk = String::new();
                if let Some(marker) = marker {
                    chunk.push_str(marker);
                    chunk.push('\n');
                }
                for part in &item.message.parts {
                    match part {
                        Part::Text { text } => {
                            let speaker = if item.message.role == Role::User {
                                "Owner"
                            } else {
                                "You"
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
                },
                PlanItem::Message {
                    message: Message::text(Role::User, text),
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
