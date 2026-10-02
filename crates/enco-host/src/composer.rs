use crate::limits::*;
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
        let (system, omitted) = self.system(input);
        let sizes = input
            .transcript
            .items
            .iter()
            .map(|item| encoded_tokens(&item.message).map(|size| (item.pos, size)))
            .collect::<Result<Vec<_>, _>>()?;
        let definitions: Vec<_> = input.tools.iter().map(|(_, spec)| spec).collect();
        let history_tokens: u32 = sizes.iter().map(|(_, size)| size).sum();
        let total = estimate_tokens(&system)
            .saturating_add(history_tokens)
            .saturating_add(encoded_tokens(&definitions)?)
            .saturating_add(budget.max_output_tokens);
        let window = budget.context_tokens;
        if total > window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100 {
            if input.compactions_left > 0 {
                let keep = window.saturating_mul(COMPACTION_TAIL_PERCENT) / 100;
                let compaction = Compaction::new(input);
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
        items.extend(
            input
                .transcript
                .items
                .iter()
                .map(|i| PlanItem::Log { pos: i.pos }),
        );
        Ok(Composition::Plan(ContextPlan {
            items,
            tools: input.tools.clone(),
            max_output_tokens: Some(budget.max_output_tokens),
            omitted,
        }))
    }
}

impl FactoryComposer {
    fn system(&self, input: &ComposeInput) -> (String, Vec<Omission>) {
        let mut text = if input.safe_mode {
            include_str!("prompts/safe_mode.md")
        } else {
            include_str!("prompts/system.md")
        }
        .trim()
        .to_string();
        text.push_str(&format!(
            "\n\n## Environment\n- Current time: {} ({})\n- Workspace: {}\n- Standing instructions: {}\n- Session: {}",
            input.now.to_rfc3339(),
            input.now.format("%A"),
            self.workspace.display(),
            self.instructions.display(),
            input.session.name
        ));
        let mut omitted = input.context.omitted.clone();
        append_instructions(&mut text, &mut omitted, input);
        append_memories(&mut text, &mut omitted, input);
        if let Some(summary) = &input.transcript.summary {
            section(&mut text, "Summary of earlier conversation", summary);
        }
        if let Some(end) = &input.previous_run_end
            && !matches!(end, RunEnd::Completed)
        {
            let reason = match end {
                RunEnd::Interrupted => "interrupted".into(),
                RunEnd::Cancelled => "cancelled".into(),
                RunEnd::BudgetExhausted => "budget exhausted".into(),
                RunEnd::Failed { failure } => format!("failed: {}", failure.message),
                RunEnd::Completed => String::new(),
            };
            section(
                &mut text,
                "Note",
                &format!(
                    "The previous run ended with: {reason}. Tool results in the history show what did and did not happen."
                ),
            );
        }
        (text, omitted)
    }
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
    section(
        text,
        "Memory (authoritative; overrides anything said earlier in the conversation)",
        memory.trim(),
    );
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
    fn new(input: &ComposeInput) -> Self {
        let mut header = String::new();
        if let Some(summary) = &input.transcript.summary {
            header.push_str(&format!("Previous summary:\n{summary}\n\n"));
        }
        header.push_str("Conversation to summarize:\n");
        let mut names = HashMap::new();
        let chunks = input
            .transcript
            .items
            .iter()
            .map(|item| {
                let mut chunk = String::new();
                for part in &item.message.parts {
                    match part {
                        Part::Text { text } => {
                            let speaker = if item.message.role == Role::User {
                                "Owner"
                            } else {
                                "Enco"
                            };
                            chunk.push_str(&format!("{speaker}: {text}\n"));
                        }
                        Part::ToolCall(call) => {
                            names.insert(call.id, call.name.as_str());
                            let arguments = clip(&call.arguments);
                            chunk
                                .push_str(&format!("Enco called {} with {arguments}\n", call.name));
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
