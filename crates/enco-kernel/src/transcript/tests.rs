use super::*;
use crate::test_support::*;

#[test]
fn clearing_discards_the_view_but_can_return_to_the_previous_reply() {
    let run = RunId::new();
    let attempt = AttemptId::new();
    let mut history = entries(vec![
        message(),
        EntryBody::RunStarted { run },
        started(attempt, AttemptPurpose::Reply, "reply-model"),
        completed(attempt, 120),
        round_end(),
        EntryBody::RunEnded {
            run,
            end: RunEnd::Completed,
        },
        consumed(EventBody::Backtrack { to: None }),
    ]);
    let cleared = conversation(&history);
    assert_eq!(
        cleared.iter().map(|entry| entry.pos).collect::<Vec<_>>(),
        vec![history[6].pos]
    );
    assert!(project(&cleared).items[0].message.parts.is_empty());
    assert!(!backtracked_to_input(&history));
    assert!(!has_compactable_round(&cleared));

    let mut restore = entries(vec![consumed(EventBody::Backtrack {
        to: Some(history[3].pos),
    })]);
    restore[0].pos.seq = Seq(8);
    history.extend(restore);
    let restored = conversation(&history);
    assert_eq!(
        restored
            .iter()
            .map(|entry| entry.pos.seq.0)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 8]
    );
    assert!(has_compactable_round(&restored));
    assert!(!backtracked_to_input(&history));

    let mut retry = entries(vec![consumed(EventBody::Backtrack {
        to: Some(history[0].pos),
    })]);
    retry[0].pos.seq = Seq(9);
    history.extend(retry);
    assert!(backtracked_to_input(&history));
    assert_eq!(conversation(&history).len(), 2);
}

#[test]
fn compaction_requests_are_derived_at_pending_round_and_compaction_boundaries() {
    let mut history = entries(vec![message(), round_end(), consumed(EventBody::Compact)]);
    assert!(compaction_requested(&conversation(&history), &[]));
    assert_eq!(project(&conversation(&history)).items.len(), 1);
    assert!(has_compactable_round(&conversation(&history)));

    // A halt leaves the request in place, before and after it is consumed.
    let inputs = [event(EventBody::Compact), event(halt())];
    assert!(compaction_requested(&conversation(&history), &inputs));
    let mut halted = history.clone();
    let mut consumed_halt = entries(vec![consumed(halt())]);
    consumed_halt[0].pos.seq = Seq(4);
    halted.extend(consumed_halt);
    assert!(compaction_requested(&conversation(&halted), &[]));
    let inputs = [event(halt()), event(EventBody::Compact)];
    assert!(compaction_requested(&conversation(&history), &inputs));
    let inputs = [event(EventBody::Backtrack { to: None })];
    assert!(!compaction_requested(&conversation(&history), &inputs));

    let mut next = entries(vec![round_end()]);
    next[0].pos.seq = Seq(4);
    history.extend(next);
    assert!(!compaction_requested(&conversation(&history), &[]));
    assert!(compaction_requested(
        &conversation(&history),
        &[event(EventBody::Compact)]
    ));

    let mut next = entries(vec![
        consumed(EventBody::Compact),
        compacted(history[3].pos),
    ]);
    next[0].pos.seq = Seq(5);
    next[1].pos.seq = Seq(6);
    history.extend(next);
    assert!(!compaction_requested(&conversation(&history), &[]));
    assert!(!has_compactable_round(&conversation(&history)));
}

#[test]
fn clear_leaves_compaction_requests_on_the_departed_branch() {
    let history = entries(vec![
        message(),
        round_end(),
        consumed(EventBody::Compact),
        consumed(EventBody::Backtrack { to: None }),
    ]);
    assert!(!compaction_requested(&conversation(&history), &[]));
    assert!(!has_compactable_round(&conversation(&history)));
}
