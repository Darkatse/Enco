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
            .saturating_add(input.budget.max_output_tokens);
        let window = input.budget.context_tokens;
        let compact_at = window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100;
        if total > compact_at {
            let keep = window.saturating_mul(COMPACTION_TAIL_PERCENT) / 100;
            let mut tail = history_tokens;
            let mut remaining = sizes.iter().peekable();
            // Both sequences follow Log order, so each message leaves the tail only once.
            for boundary in &input.transcript.round_ends {
                while let Some((_, size)) = remaining.next_if(|(pos, _)| pos <= boundary) {
                    tail -= size;
                }
                if tail <= keep {
                    return Ok(Composition::Compact {
                        upto: *boundary,
                        plan: compaction(input, *boundary),
                    });
                }
            }
            if total > input.budget.context_tokens {
                return Err(ComposeError::ContextOverflow {
                    needed: total,
                    window: input.budget.context_tokens,
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
            max_output_tokens: Some(input.budget.max_output_tokens),
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
    let budget = input.budget.context_tokens.saturating_mul(MEMORY_PERCENT) / 100;
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

fn compaction(input: &ComposeInput, upto: LogPos) -> ContextPlan {
    let mut text = String::new();
    let mut names = HashMap::new();
    if let Some(summary) = &input.transcript.summary {
        text.push_str(&format!("Previous summary:\n{summary}\n\n"));
    }
    text.push_str("Conversation to summarize:\n");
    for item in input.transcript.items.iter().filter(|i| i.pos <= upto) {
        for part in &item.message.parts {
            match part {
                Part::Text { text: body } => {
                    let speaker = if item.message.role == Role::User {
                        "Owner"
                    } else {
                        "Enco"
                    };
                    text.push_str(&format!("{speaker}: {body}\n"));
                }
                Part::ToolCall(call) => {
                    names.insert(call.id, call.name.as_str());
                    text.push_str(&format!(
                        "Enco called {} with {}\n",
                        call.name, call.arguments
                    ));
                }
                Part::ToolResult(result) => {
                    let name = names.get(&result.call).copied().unwrap_or("tool");
                    let end = result.content.floor_char_boundary(COMPACTION_RESULT_BYTES);
                    let preview = &result.content[..end];
                    text.push_str(&format!("Result of {name}: {preview}\n"));
                }
                Part::Extension(_) => {}
            }
        }
    }
    text.push_str("\nWrite the summary now.");
    ContextPlan {
        items: vec![
            PlanItem::Message {
                message: Message::text(Role::System, include_str!("prompts/compaction.md")),
            },
            PlanItem::Message {
                message: Message::text(Role::User, text),
            },
        ],
        tools: vec![],
        max_output_tokens: Some(input.budget.max_output_tokens.min(COMPACTION_OUTPUT_TOKENS)),
        omitted: vec![],
    }
}

fn encoded_tokens(value: &impl serde::Serialize) -> Result<u32, ComposeError> {
    serde_json::to_string(value)
        .map(|text| estimate_tokens(&text))
        .map_err(|e| ComposeError::Invalid(e.to_string()))
}
