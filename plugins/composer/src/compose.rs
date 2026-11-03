//! Selects reply content and asks for compaction when the model's window is nearly full.
use crate::composer::*;
use crate::{
    Options, compaction::Compaction, context, estimate_tokens, history, limits::*, tokens,
};
use enco_sdk::types::Failure;

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
    let system_tokens = estimate_tokens(&head.text);
    let history = history::annotated(input, timezone)?;
    let selected_images = crate::images::selected(input, &mut omitted);
    let (items, reused_prefix_len) = history::reply_items(
        input,
        PlanItem::Message(head),
        &history,
        &selected_images,
        &mut memory_budget,
        &mut omitted,
    );
    let (sizes, trailing) = history::sizes(&items[1..], &input.transcript)?;
    let history_tokens = sizes
        .iter()
        .fold(trailing, |total, (_, size)| total.saturating_add(*size));
    let tools: Vec<_> = input
        .capabilities
        .iter()
        .map(|capability| capability.id.clone())
        .collect();
    let total = request_tokens(
        input,
        &items,
        reused_prefix_len,
        &tools,
        system_tokens,
        history_tokens,
    )?;
    let window = budget.context_tokens;
    let threshold = u64::from(window) * u64::from(options.compact_at) / 100;
    if (input.compaction_requested || u64::from(total) > threshold)
        && input.compactions_left > 0
        && let Some(budget) = &input.compaction
    {
        // The retained tail scales with the threshold: 3/8 of it, so 30% of the window at 80%.
        let keep = if input.compaction_requested {
            0
        } else {
            threshold * 3 / 8
        };
        let compaction = Compaction::new(input, &history, budget)?;
        if let Some(upto) = compaction.boundary(input, &sizes, history_tokens, keep) {
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
    Ok(Composition::Reply(Plan {
        items,
        tools,
        max_output_tokens: Some(budget.max_output_tokens),
        omitted,
    }))
}

/// The request's size in tokens, output budget included.
///
/// When the previous plan is reused with the same tools, its reported input tokens measure the
/// whole prefix, head, images and tool definitions included, and only the appended items are
/// estimated; otherwise everything is estimated.
fn request_tokens(
    input: &ComposeInput,
    items: &[PlanItem],
    reused_prefix_len: Option<usize>,
    tools: &[CapabilityId],
    system_tokens: u32,
    history_tokens: u32,
) -> Result<u32, Failure> {
    let input_tokens = if let (Some(prefix_len), Some(previous), Some(usage)) =
        (reused_prefix_len, &input.previous, &input.previous_usage)
        && previous.tools == tools
    {
        u32::try_from(usage.input_tokens)
            .unwrap_or(u32::MAX)
            .saturating_add(history::appended_tokens(
                &items[prefix_len..],
                &input.transcript,
            )?)
    } else {
        let definitions = tokens(
            input
                .capabilities
                .iter()
                .map(|capability| {
                    let spec = &capability.spec;
                    spec.name.len()
                        + spec.description.len()
                        + spec.input_schema.len()
                        + PART_OVERHEAD_BYTES
                })
                .sum(),
        );
        system_tokens
            .saturating_add(history_tokens)
            .saturating_add(definitions)
    };
    Ok(input_tokens.saturating_add(input.reply.budget.max_output_tokens))
}
