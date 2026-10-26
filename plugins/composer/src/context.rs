//! Renders the stable request head and records which instruction and memory sources it includes.
use crate::composer::*;
use crate::{estimate_tokens, limits::*, recovery};
use chrono::Utc;
use chrono_tz::Tz;
use enco_sdk::types::Role;
use std::path::Path;

pub(super) fn system(
    input: &ComposeInput,
    timezone: Tz,
    pinned: &[&Memory],
    omitted: &mut Vec<Omission>,
) -> Inline {
    let mut text = if input.safe_mode.is_some() {
        include_str!("prompts/safe_mode.md")
    } else {
        include_str!("prompts/system.md")
    }
    .trim()
    .to_string();
    let home = Path::new(&input.home);
    text.push_str(&format!(
        "\n\n## Environment\n- Home: {} (config.toml and plugins/)\n- Manual: {} (start with README.md; generated, so edits are lost)\n- Workspace: {}\n- Standing instructions: {}\n- Session: {}\n- Time zone: {} (UTC{})",
        home.display(), home.join("manual").display(), home.join("workspace").display(), home.join("AGENTS.md").display(), input.session.name,
        input.timezone, Utc::now().with_timezone(&timezone).format("%:z")
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
    if let Some(instructions) = &input.instructions {
        let budget = input
            .reply
            .budget
            .context_tokens
            .saturating_mul(INSTRUCTION_PERCENT)
            / 100;
        if estimate_tokens(instructions) <= budget {
            section(&mut text, "Standing instructions (AGENTS.md)", instructions);
            sources.push(Source {
                id: "instructions:AGENTS.md".into(),
                hash: enco_sdk::source_hash(instructions),
            });
        } else {
            omitted.push(Omission {
                source: "instructions:AGENTS.md".into(),
                reason: format!("instructions exceed {INSTRUCTION_PERCENT}% of the context window"),
            });
        }
    }
    append_memories(&mut text, &mut sources, pinned, "## Your pinned memories");
    if let Some(summary) = &input.transcript.summary {
        section(
            &mut text,
            "Your summary of the earlier conversation",
            summary,
        );
    }
    Inline {
        role: Role::System,
        text,
        sources,
    }
}

pub(super) fn memory_source(memory: &Memory) -> Source {
    Source {
        id: format!("memory:{}", memory.id),
        hash: enco_sdk::source_hash(&memory.text),
    }
}

pub(super) fn select_memories<'a>(
    candidates: impl Iterator<Item = &'a Memory>,
    remaining: &mut u32,
    omitted: &mut Vec<Omission>,
) -> Vec<&'a Memory> {
    let mut selected = Vec::new();
    for candidate in candidates {
        let size = estimate_tokens(&candidate.text);
        if size <= *remaining {
            *remaining -= size;
            selected.push(candidate);
        } else {
            omitted.push(Omission {
                source: format!("memory:{}", candidate.id),
                reason: format!("memory budget ({MEMORY_PERCENT}% of the context window) exceeded"),
            });
        }
    }
    selected
}

pub(super) fn append_memories(
    text: &mut String,
    sources: &mut Vec<Source>,
    memories: &[&Memory],
    heading: &str,
) {
    if memories.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(heading);
    for memory in memories {
        text.push_str(&format!("\n- {} (id: {})", memory.text, memory.id));
        sources.push(memory_source(memory));
    }
}

fn section(text: &mut String, title: &str, body: &str) {
    if !body.is_empty() {
        text.push_str(&format!("\n\n## {title}\n{body}"));
    }
}
