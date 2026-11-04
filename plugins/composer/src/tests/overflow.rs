//! A request its target turned away shrinks in proportion to the refused one, cutting at Round
//! ends before this Run.
use super::delegation::{call, child, statuses};
use super::measurement::rounds;
use super::*;
use enco_sdk::types::Overflow;

/// Ten earlier Runs of about 10k estimated tokens each, then the input that starts this Run.
fn crowded() -> ComposeInput {
    let mut input = rounds(10, 30_000);
    input.reply.budget.context_tokens = 1_000_000;
    input.compaction = None;
    input
}

/// Two short earlier Runs, then this Run's input of `bytes`.
fn starting(bytes: usize) -> ComposeInput {
    let mut input = rounds(2, 300);
    input.reply.budget.context_tokens = 1_000_000;
    input.compaction = None;
    *input.transcript.items.last_mut().unwrap() =
        incoming(9, "2026-10-02T15:00:00Z", &"z".repeat(bytes));
    input
}

/// Turn the request `input` composes away, with the numbers the service gave.
fn refuse(input: &mut ComposeInput, counted: Option<u64>, accepted: Option<u64>) -> Plan {
    let plan = reply(input);
    input.reply.overflowed = Some(Overflowed {
        plan: plan.clone(),
        overflow: Overflow {
            counted,
            accepted,
            message: "too long".into(),
        },
    });
    plan
}

/// The Log positions a plan still carries.
fn carried(plan: &Plan) -> Vec<u64> {
    plan.items
        .iter()
        .filter_map(|item| match item {
            PlanItem::Log(log) => Some(log.pos.seq),
            PlanItem::Message(_) => None,
        })
        .collect()
}

#[test]
fn the_service_count_sets_how_much_history_stays() {
    let mut input = crowded();
    let refused = refuse(&mut input, Some(3_000), Some(1_000));
    let plan = reply(&input);
    // A third of the refused request, less the margin, holds the last two earlier Runs.
    assert_eq!(carried(&plan), [33, 34, 37, 38, 41]);
    assert_ne!(plan.items, refused.items);
    // The note to the model follows the head, and the plan records where the omission ends.
    assert!(matches!(&plan.items[1], PlanItem::Message(note) if note.role == Role::User));
    let sources: Vec<_> = plan.omitted.iter().map(|omitted| &omitted.source).collect();
    assert_eq!(sources, ["history:1:31"]);
}

#[test]
fn without_a_count_the_request_halves() {
    let mut input = crowded();
    refuse(&mut input, None, Some(2_000_000));
    let plan = reply(&input);
    assert_eq!(carried(&plan), [25, 26, 29, 30, 33, 34, 37, 38, 41]);
}

#[test]
fn a_later_round_keeps_this_runs_input_and_calls() {
    let mut input = crowded();
    // This Run's first Round called a tool, so the next starts with nothing new after its end.
    input.transcript.items.extend(call(42));
    input.transcript.round_ends.push(pos(44));
    refuse(&mut input, Some(3_000), Some(1_000));
    assert_eq!(carried(&reply(&input)), [33, 34, 37, 38, 41, 42, 43]);
}

#[test]
fn an_input_arriving_mid_run_leaves_the_runs_earlier_rounds_in_place() {
    let mut input = starting(30_000);
    input.transcript.items.extend(call(10));
    input.transcript.round_ends.push(pos(12));
    input
        .transcript
        .items
        .push(incoming(13, "2026-10-02T15:05:00Z", "One more thing"));
    refuse(&mut input, Some(3_000), Some(1_000));
    // Leaving out the Run's first Round would fit, but its calls record effects that happened.
    let failure = Plugin::compose("{}".into(), input).unwrap_err();
    assert_eq!(failure.code, "context.overflow");
}

#[test]
fn this_run_beyond_the_share_fails_as_an_overflow() {
    let mut input = starting(300_000);
    refuse(&mut input, Some(3_000), Some(1_000));
    let failure = Plugin::compose("{}".into(), input).unwrap_err();
    assert_eq!(failure.code, "context.overflow");
}

#[test]
fn a_shrunk_request_keeps_the_status_and_the_next_one_extends_it() {
    let mut input = crowded();
    input.children.push(child());
    refuse(&mut input, Some(3_000), Some(1_000));
    let shrunk = reply(&input);
    assert_eq!(statuses(&shrunk, 0).len(), 1);

    // The shrunk request was answered with a call; the next Round adds to it.
    input.reply.overflowed = None;
    input.previous = Some(shrunk.clone());
    input.transcript.items.extend(call(42));
    input.transcript.round_ends.push(pos(44));
    let next = reply(&input);
    assert!(next.items.starts_with(&shrunk.items));
    assert_eq!(carried(&next), [33, 34, 37, 38, 41, 42, 43]);
}

#[test]
fn history_cut_away_is_summarized_when_a_compaction_is_available() {
    let mut input = crowded();
    input.compaction = Some(summarizer(1_000_000, 512));
    refuse(&mut input, Some(3_000), Some(1_000));
    let Composition::Compact(compaction) = Plugin::compose("{}".into(), input.clone()).unwrap()
    else {
        panic!("expected compaction")
    };
    assert_eq!(compaction.upto, pos(31));

    // The summarizing model turns its request away too: it summarizes an earlier stretch.
    input.compaction.as_mut().unwrap().overflowed = Some(Overflowed {
        plan: compaction.plan,
        overflow: Overflow {
            counted: None,
            accepted: None,
            message: "too long".into(),
        },
    });
    let Composition::Compact(smaller) = Plugin::compose("{}".into(), input).unwrap() else {
        panic!("expected compaction")
    };
    assert!(smaller.upto.seq < 31, "{:?}", smaller.upto);
}
