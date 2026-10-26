//! Selects reply content and asks for compaction when the model's window is nearly full.
use crate::composer::*;
use crate::{compaction::Compaction, context, estimate_tokens, history, limits::*, tokens};
use enco_sdk::types::Failure;

pub(super) fn compose(input: &ComposeInput) -> Result<Composition, Failure> {
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
    let items = history::reply_items(
        input,
        PlanItem::Message(head),
        &history,
        &selected_images,
        &mut memory_budget,
        &mut omitted,
    );
    let sizes = history::sizes(&items[1..], &input.transcript)?;
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
    let history_tokens: u32 = sizes.iter().map(|(_, size)| size).sum();
    let total = system_tokens
        .saturating_add(history_tokens)
        .saturating_add(definitions)
        .saturating_add(budget.max_output_tokens);
    let window = budget.context_tokens;
    if total > window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100 {
        if input.compactions_left > 0
            && let Some(budget) = &input.compaction
        {
            let keep = window.saturating_mul(COMPACTION_TAIL_PERCENT) / 100;
            let compaction = Compaction::new(input, &history, budget);
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
    }
    Ok(Composition::Reply(Plan {
        items,
        tools: input
            .capabilities
            .iter()
            .map(|capability| capability.id.clone())
            .collect(),
        max_output_tokens: Some(budget.max_output_tokens),
        omitted,
    }))
}
