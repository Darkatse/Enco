//! Selects reply content and asks for compaction when the model's window is nearly full.
use crate::composer::*;
use crate::{Options, compaction::Compaction, context, history, limits::*, tokens};
use enco_sdk::types::Failure;
use std::collections::BTreeSet;

pub(super) fn compose(input: &ComposeInput, options: &Options) -> Result<Composition, Failure> {
    let timezone = input
        .timezone
        .parse()
        .map_err(|error| crate::failed(format!("time zone {}: {error}", input.timezone)))?;
    let budget = &input.reply.budget;
    let mut omitted = vec![];
    let mut memory_budget = budget.context_tokens.saturating_mul(MEMORY_PERCENT) / 100;
    let pinned = context::select_memories(
        input.memories.iter().filter(|memory| memory.pinned),
        &mut memory_budget,
        &mut omitted,
    );
    let head = context::system(input, timezone, &pinned, &mut omitted);
    let history = history::annotated(input, timezone)?;
    let selected_images = crate::images::selected(input, &mut omitted);
    let renderer = Renderer {
        input,
        head,
        selected_images,
        memory_budget,
        omitted,
    };
    if let Some(overflowed) = &input.reply.overflowed {
        return crate::overflow::shrink(input, &history, &renderer, overflowed);
    }
    let rendering = renderer.render(input.previous.as_ref(), &history)?;
    let total = request_tokens(input, &rendering)?;
    let window = budget.context_tokens;
    let threshold = u64::from(window) * u64::from(options.compact_at) / 100;
    if (input.compaction_requested || u64::from(total) > threshold)
        && let Some(target) = &input.compaction
    {
        // The retained tail scales with the threshold: 3/8 of it, so 30% of the window at 80%.
        let keep = if input.compaction_requested {
            0
        } else {
            threshold * 3 / 8
        };
        let budget = crate::overflow::compaction_budget(input, target)?;
        let compaction = Compaction::new(input, &history, &budget)?;
        if let Some(upto) =
            compaction.boundary(input, &rendering.sizes, rendering.history_tokens, keep)
        {
            return Ok(Composition::Compact(crate::composer::Compaction {
                plan: compaction.plan(&upto),
                upto,
            }));
        }
    }
    if total > window {
        return Err(enco_sdk::protocol::failure(
            "context.overflow",
            format!("context needs approximately {total} tokens, but the window is {window}"),
            false,
        ));
    }
    Ok(reply(input, rendering.items, rendering.omitted))
}

/// What every reply rendering of this composition starts from, fixed before its history is
/// chosen: the head, the images to send, and the memory budget and omissions left after the head.
pub(super) struct Renderer<'a> {
    input: &'a ComposeInput,
    head: Inline,
    selected_images: BTreeSet<(u64, u64)>,
    memory_budget: u32,
    omitted: Vec<Omission>,
}

impl Renderer<'_> {
    /// The reply request over `history`, extending `previous` while it still applies.
    pub(super) fn render(
        &self,
        previous: Option<&Plan>,
        history: &[(&TranscriptItem, String)],
    ) -> Result<Rendering, Failure> {
        let input = self.input;
        let mut memory_budget = self.memory_budget;
        let mut omitted = self.omitted.clone();
        let (items, reused) = history::reply_items(
            input,
            previous,
            PlanItem::Message(self.head.clone()),
            history,
            &self.selected_images,
            &mut memory_budget,
            &mut omitted,
        );
        let (sizes, trailing) = history::sizes(&items[1..], &input.transcript)?;
        let history_tokens = sizes
            .iter()
            .fold(trailing, |total, (_, size)| total.saturating_add(*size));
        Ok(Rendering {
            head_tokens: history::items_tokens(&items[..1], &input.transcript)?,
            items,
            sizes,
            history_tokens,
            definitions: definitions(input.capabilities.iter()),
            reused,
            omitted,
        })
    }
}

/// A reply request as rendered, with the estimates a decision about its size needs.
///
/// Every item is estimated as a recorded plan's are, so the head, history and definitions add up
/// to the estimate the same request gets once it is recorded and refused.
pub(super) struct Rendering {
    /// The request's messages, its head first.
    pub items: Vec<PlanItem>,
    /// Each Log item's size with the notes just before it, in Log order.
    pub sizes: Vec<(LogPos, u32)>,
    /// Every item after the head, the notes after the last Log item included.
    pub history_tokens: u32,
    /// The head, envelope included.
    pub head_tokens: u32,
    /// The tool definitions the request discloses.
    pub definitions: u32,
    /// How many leading items repeat the previous plan, when the request extends it.
    pub reused: Option<usize>,
    /// What the request leaves out, with why.
    pub omitted: Vec<Omission>,
}

impl Rendering {
    /// The head and tool definitions, which every request carries whatever history it keeps.
    pub(super) fn fixed_tokens(&self) -> u32 {
        self.head_tokens.saturating_add(self.definitions)
    }
}

/// The reply plan with these items, disclosing every capability.
pub(super) fn reply(
    input: &ComposeInput,
    items: Vec<PlanItem>,
    omitted: Vec<Omission>,
) -> Composition {
    Composition::Reply(Plan {
        items,
        tools: tools(input),
        max_output_tokens: input.reply.budget.max_output_tokens,
        omitted,
    })
}

fn tools(input: &ComposeInput) -> Vec<CapabilityId> {
    input
        .capabilities
        .iter()
        .map(|capability| capability.id.clone())
        .collect()
}

/// The estimated size of these tools' definitions.
pub(super) fn definitions<'a>(capabilities: impl Iterator<Item = &'a Capability>) -> u32 {
    tokens(
        capabilities
            .map(|capability| {
                let spec = &capability.spec;
                spec.name.len()
                    + spec.description.len()
                    + spec.input_schema.len()
                    + PART_OVERHEAD_BYTES
            })
            .sum(),
    )
}

/// The request's size in tokens, output budget included.
///
/// When the previous plan is reused with the same tools, its reported input tokens measure the
/// whole prefix, head, images and tool definitions included, and only the appended items are
/// estimated; otherwise everything is estimated.
fn request_tokens(input: &ComposeInput, rendering: &Rendering) -> Result<u32, Failure> {
    let input_tokens = if let (Some(prefix_len), Some(previous), Some(usage)) = (
        rendering.reused,
        &input.previous,
        &input.reply.previous_usage,
    ) && previous.tools == tools(input)
    {
        u32::try_from(usage.input_tokens)
            .unwrap_or(u32::MAX)
            .saturating_add(history::items_tokens(
                &rendering.items[prefix_len..],
                &input.transcript,
            )?)
    } else {
        rendering
            .fixed_tokens()
            .saturating_add(rendering.history_tokens)
    };
    Ok(input_tokens.saturating_add(input.reply.budget.max_output_tokens))
}
