//! Shrinks a request its target turned away as too large, in proportion to the refused one.
//!
//! The refusal shows the estimate ran low for this target, so the next request aims for a share
//! of the refused one rather than an absolute size: the service's own count sets the share when
//! it states one, half otherwise. History cut away is summarized when a compaction is available
//! and otherwise left out with its reason, so the Session keeps working.
use crate::compose::{Renderer, Rendering, definitions, reply};
use crate::composer::*;
use crate::{compaction::Compaction, cut, history, key, limits::*, tokens};
use enco_sdk::types::{Failure, Overflow, Role};

/// The composition after the reply target's combination turned `overflowed` away.
///
/// It renders afresh, since the presentation it would extend was turned away, and keeps the head,
/// the tool definitions and the most recent complete Rounds that fit the share of the refused
/// request, cutting only at Round ends.
pub(super) fn shrink(
    input: &ComposeInput,
    history: &[(&TranscriptItem, String)],
    renderer: &Renderer,
    overflowed: &Overflowed,
) -> Result<Composition, Failure> {
    let goal = scaled(plan_tokens(&overflowed.plan, input)?, &overflowed.overflow);
    let rendering = renderer.render(None, history)?;
    let fixed = rendering.fixed_tokens();
    if fixed.saturating_add(rendering.history_tokens) <= goal {
        return Ok(reply(input, rendering.items, rendering.omitted));
    }
    if let Some(target) = &input.compaction {
        let budget = compaction_budget(input, target)?;
        // The summary replaces the history it covers, up to the summarizing model's output limit.
        let keep = goal.saturating_sub(fixed.saturating_add(budget.max_output_tokens));
        let compaction = Compaction::new(input, history, &budget)?;
        if let Some(upto) = compaction.boundary(
            input,
            &rendering.sizes,
            rendering.history_tokens,
            keep.into(),
        ) {
            return Ok(Composition::Compact(crate::composer::Compaction {
                plan: compaction.plan(&upto),
                upto,
            }));
        }
    }
    omit(
        input,
        history,
        renderer,
        &rendering,
        goal,
        &overflowed.overflow,
    )
}

/// The reply that leaves out the earliest history before this Run, rendered over what it keeps,
/// with a note to the model right after the head; `context.overflow` when this Run alone does not
/// fit.
fn omit(
    input: &ComposeInput,
    history: &[(&TranscriptItem, String)],
    renderer: &Renderer,
    rendering: &Rendering,
    goal: u32,
    overflow: &Overflow,
) -> Result<Composition, Failure> {
    let note = note(input, overflow);
    let room = goal.saturating_sub(rendering.fixed_tokens().saturating_add(tokens(
        note.len() + MESSAGE_OVERHEAD_BYTES + PART_OVERHEAD_BYTES,
    )));
    let cuts = cut::run_start(&input.transcript)
        .map(|start| {
            cut::cuts(
                &input.transcript,
                &rendering.sizes,
                rendering.history_tokens,
                start,
            )
        })
        .into_iter()
        .flatten();
    for (end, tail) in cuts {
        // What is kept renders at least as large as its tail, since composition adds its own notes.
        if tail > room {
            continue;
        }
        let kept: Vec<_> = history
            .iter()
            .filter(|(item, _)| key(&item.pos) > key(&end))
            .cloned()
            .collect();
        let shrunk = renderer.render(None, &kept)?;
        if shrunk.history_tokens > room {
            continue;
        }
        let mut items = shrunk.items;
        items.insert(1, crate::inline(Role::User, note));
        let mut omitted = shrunk.omitted;
        omitted.push(Omission {
            source: format!("history:{}:{}", end.epoch, end.seq),
            reason: format!(
                "the service turned a larger request away as too long ({}), so this request keeps to {goal} estimated input tokens",
                detail(overflow)
            ),
        });
        return Ok(reply(input, items, omitted));
    }
    Err(enco_sdk::protocol::failure(
        "context.overflow",
        format!(
            "the service turned the request away as too large ({}), and the current task alone, from the message that started it, exceeds the {goal} estimated input tokens a new request may take",
            detail(overflow)
        ),
        false,
    ))
}

/// The summarizing model's limits for this composition: after its combination turned a request
/// away, a reply or a summary, its window shrinks in proportion to that request.
pub(super) fn compaction_budget(
    input: &ComposeInput,
    target: &CompactionTarget,
) -> Result<Budget, Failure> {
    let Some(overflowed) = &target.overflowed else {
        return Ok(target.budget);
    };
    let accepted = scaled(plan_tokens(&overflowed.plan, input)?, &overflowed.overflow);
    Ok(Budget {
        context_tokens: accepted
            .saturating_add(target.budget.max_output_tokens)
            .min(target.budget.context_tokens),
        max_output_tokens: target.budget.max_output_tokens,
    })
}

/// The share of a refused request's estimated input the next request aims for.
fn scaled(refused: u32, overflow: &Overflow) -> u32 {
    let refused = u64::from(refused);
    let goal = match (overflow.counted, overflow.accepted) {
        (Some(counted), Some(accepted)) if counted > 0 => {
            refused.saturating_mul(accepted) / counted * OVERFLOW_MARGIN_PERCENT / 100
        }
        _ => refused * OVERFLOW_DEFAULT_PERCENT / 100,
    };
    u32::try_from(goal).unwrap_or(u32::MAX)
}

/// The estimated input of a recorded plan: its messages and the definitions it discloses, counted
/// as a rendering counts the request it renders.
fn plan_tokens(plan: &Plan, input: &ComposeInput) -> Result<u32, Failure> {
    let messages = history::items_tokens(&plan.items, &input.transcript)?;
    let disclosed = definitions(
        input
            .capabilities
            .iter()
            .filter(|capability| plan.tools.contains(&capability.id)),
    );
    Ok(messages.saturating_add(disclosed))
}

/// What the model reads in place of the history left out: what the service said, this model's
/// configured window when the service named its limit, and where the full record is.
fn note(input: &ComposeInput, overflow: &Overflow) -> String {
    let mut text = format!(
        "The earlier conversation is left out of this request: the service behind this model turned a larger request away as too long ({}).",
        detail(overflow)
    );
    if overflow.accepted.is_some() {
        let budget = &input.reply.budget;
        text.push_str(&format!(
            " This model is configured with a {}-token window, {} of them for output.",
            budget.context_tokens, budget.max_output_tokens
        ));
    }
    text.push_str(&format!(
        " The full conversation is still on record; `enco log --session {}` shows it.",
        input.session.name
    ));
    text
}

/// What the service said about the refused request's size.
fn detail(overflow: &Overflow) -> String {
    match (overflow.counted, overflow.accepted) {
        (Some(counted), Some(accepted)) => {
            format!("it counted {counted} input tokens and accepts at most {accepted}")
        }
        (Some(counted), None) => format!("it counted {counted} input tokens"),
        (None, Some(accepted)) => format!("it accepts at most {accepted} input tokens"),
        (None, None) => "it gave no numbers".into(),
    }
}
