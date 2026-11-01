//! Renders the stable request head and records which instruction and memory sources it includes.
use crate::composer::*;
use crate::{estimate_tokens, limits::*, recovery};
use chrono::Utc;
use chrono_tz::Tz;
use enco_sdk::types::Role;
use std::path::Path;

/// The owner instructions, relative to the home.
const OWNER_FILE: &str = "AGENTS.md";

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
    let profile_file = format!("profiles/{}.md", input.session.profile);
    // Without the owner's context the owner instructions are not this Session's to read or edit.
    let owner_line = if input.owner_context {
        format!(
            "\n- Owner instructions: {}",
            home.join(OWNER_FILE).display()
        )
    } else {
        String::new()
    };
    text.push_str(&format!(
        "\n\n## Environment\n- Home: {} (config.toml, profiles/ and plugins/)\n- Manual: {} (start with README.md; generated, so edits are lost)\n- Working directory: {}{owner_line}\n- Profile instructions: {}\n- Session: {}\n- Time zone: {} (UTC{})",
        home.display(), home.join("manual").display(), input.session.working_dir.path, home.join(&profile_file).display(), input.session.name,
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
    let budget = input
        .reply
        .budget
        .context_tokens
        .saturating_mul(INSTRUCTION_PERCENT)
        / 100;
    for (label, file, instructions) in [
        ("Owner instructions", OWNER_FILE, &input.owner_instructions),
        (
            "Profile instructions",
            profile_file.as_str(),
            &input.profile_instructions,
        ),
    ] {
        if let Some(instructions) = instructions {
            append_instructions(
                &mut text,
                &mut sources,
                omitted,
                budget,
                label,
                file,
                instructions,
            );
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

/// Renders one file of standing instructions within its own budget, or records why it is left out.
fn append_instructions(
    text: &mut String,
    sources: &mut Vec<Source>,
    omitted: &mut Vec<Omission>,
    budget: u32,
    label: &str,
    file: &str,
    instructions: &str,
) {
    let source = format!("instructions:{file}");
    if estimate_tokens(instructions) <= budget {
        section(text, &format!("{label} ({file})"), instructions);
        sources.push(Source {
            id: source,
            hash: enco_sdk::source_hash(instructions),
        });
    } else {
        omitted.push(Omission {
            source,
            reason: format!("instructions exceed {INSTRUCTION_PERCENT}% of the context window"),
        });
    }
}
