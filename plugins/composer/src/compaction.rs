//! Planning a compaction: how much history to summarize, and the request that summarizes it.
use crate::composer::*;
use crate::{estimate_tokens, key, limits::*};
use enco_sdk::types::Role;
use std::collections::HashMap;

const COMPACTION_PROMPT: &str = include_str!("prompts/compaction.md");
const COMPACTION_FOOTER: &str = "\nWrite the summary now.";

/// The history as the summarizer reads it.
///
/// Choosing the boundary and building the plan share one rendering, so the plan fits the window
/// the boundary was chosen for.
pub(super) struct Compaction {
    window: u32,
    header: String,
    /// One rendered chunk per Transcript item, in Log order.
    chunks: Vec<(LogPos, String)>,
    max_output_tokens: u32,
    /// Tokens the request needs besides the chunks: prompt, header, footer and output.
    overhead: u32,
}

impl Compaction {
    pub(super) fn new(
        input: &ComposeInput,
        history: &[(&TranscriptItem, String)],
        budget: &Budget,
    ) -> Self {
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
                        Part::Text(text) => {
                            let speaker = match item.event.as_ref().map(|event| &event.body) {
                                Some(EventBody::UserMessage(_)) => "Owner",
                                Some(EventBody::Reminder(_)) => "Reminder",
                                Some(
                                    EventBody::GenerationRolledBack(_) | EventBody::Backtrack(_),
                                ) => "Notice",
                                None => "You",
                            };
                            chunk.push_str(&format!("{speaker}: {text}\n"));
                        }
                        Part::ToolCall(call) => {
                            names.insert(call.id.as_str(), call.name.as_str());
                            let arguments = clip(&call.arguments);
                            chunk.push_str(&format!("You called {} with {arguments}\n", call.name));
                        }
                        Part::ToolResult(result) => {
                            let name = names
                                .get(result.call_id.as_str())
                                .copied()
                                .unwrap_or("tool");
                            chunk.push_str(&format!(
                                "Result of {name}: {}\n",
                                clip(&result.content)
                            ));
                        }
                        Part::Extension(_) | Part::Image(_) => {}
                    }
                }
                (item.pos, chunk)
            })
            .collect();
        let max_output_tokens = budget.max_output_tokens;
        // Estimating parts separately never undercounts their concatenation.
        let overhead = estimate_tokens(COMPACTION_PROMPT)
            .saturating_add(estimate_tokens(&header))
            .saturating_add(estimate_tokens(COMPACTION_FOOTER))
            .saturating_add(max_output_tokens);
        Self {
            window: budget.context_tokens,
            header,
            chunks,
            max_output_tokens,
            overhead,
        }
    }

    /// The Round end up to which compaction summarizes the history.
    ///
    /// It is the earliest Round end that leaves at most `keep` tokens of history, or, when
    /// summarizing that far would not fit the compaction window, the latest Round end that does.
    /// Each summary carries the previous one forward, so a partial compaction is progress.
    pub(super) fn boundary(
        &self,
        input: &ComposeInput,
        sizes: &[(LogPos, u32)],
        history_tokens: u32,
        keep: u32,
    ) -> Option<LogPos> {
        let first = key(&self.chunks.first()?.0);
        let window = self.window;
        let mut tail = history_tokens;
        let mut needed = self.overhead;
        let mut sizes = sizes.iter().peekable();
        let mut chunks = self.chunks.iter().peekable();
        let mut fitting = None;
        // All three sequences follow Log order, so each item is counted once.
        // A compaction-only Round has no visible content to summarize again.
        for end in input
            .transcript
            .round_ends
            .iter()
            .filter(|end| key(end) >= first)
        {
            while let Some((_, size)) = sizes.next_if(|(pos, _)| key(pos) <= key(end)) {
                tail -= size;
            }
            while let Some((_, chunk)) = chunks.next_if(|(pos, _)| key(pos) <= key(end)) {
                needed = needed.saturating_add(estimate_tokens(chunk));
            }
            if needed > window {
                break;
            }
            fitting = Some(*end);
            if tail <= keep {
                break;
            }
        }
        fitting
    }

    pub(super) fn plan(&self, upto: &LogPos) -> Plan {
        let mut text = self.header.clone();
        for (_, chunk) in self
            .chunks
            .iter()
            .take_while(|(pos, _)| key(pos) <= key(upto))
        {
            text.push_str(chunk);
        }
        text.push_str(COMPACTION_FOOTER);
        Plan {
            items: vec![
                crate::inline(Role::System, COMPACTION_PROMPT),
                crate::inline(Role::User, text),
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
