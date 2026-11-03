//! Measured prefixes, compaction thresholds and owner-requested compaction follow one planning path.
use super::*;
use enco_sdk::types::Usage;

fn rounds(count: u64, bytes: usize) -> ComposeInput {
    let mut input = input();
    for round in 0..count {
        let seq = round * 4 + 1;
        input
            .transcript
            .items
            .push(incoming(seq, "2026-10-02T14:00:00Z", &"x".repeat(bytes)));
        input.transcript.items.push(TranscriptItem {
            pos: pos(seq + 1),
            message: Message {
                role: Role::Assistant,
                parts: vec![Part::Text("Done.".into())],
            },
            event: None,
        });
        input.transcript.round_ends.push(pos(seq + 2));
    }
    input
}

fn usage(tokens: u64) -> Option<Usage> {
    Some(Usage {
        input_tokens: tokens,
        output_tokens: 10,
        cached_input_tokens: Some(tokens / 2),
    })
}

#[test]
fn measurements_replace_the_whole_prefix_estimate_but_not_appended_items() {
    let mut small = rounds(1, 60);
    small.previous = Some(reply(&small));
    small.previous_usage = usage(81_000);
    assert!(matches!(
        Plugin::compose("{}".into(), small.clone()).unwrap(),
        Composition::Compact(_)
    ));
    small.previous_usage = usage(u64::MAX);
    assert!(matches!(
        Plugin::compose("{}".into(), small).unwrap(),
        Composition::Compact(_)
    ));

    let mut large = rounds(1, 90_000);
    large.reply.budget.context_tokens = 200_000;
    large.compaction.as_mut().unwrap().context_tokens = 200_000;
    let previous = reply(&large);
    large.reply.budget.context_tokens = 30_000;
    large.previous = Some(previous.clone());
    large.previous_usage = usage(100);
    let plan = reply(&large);
    assert_eq!(plan.items, previous.items);

    large
        .transcript
        .items
        .push(incoming(5, "2026-10-02T14:00:00Z", &"y".repeat(75_000)));
    assert!(matches!(
        Plugin::compose("{}".into(), large).unwrap(),
        Composition::Compact(_)
    ));
}

#[test]
fn changed_tools_missing_usage_and_new_series_use_full_estimates() {
    let mut small = rounds(1, 60);
    small.capabilities = vec![delegation::capability("shell")];
    small.previous = Some(reply(&small));
    small.previous_usage = usage(81_000);
    small.capabilities.clear();
    reply(&small);

    small.capabilities = vec![delegation::capability("shell")];
    small.transcript.summary = Some("A new summary replaces the earlier conversation.".into());
    small.transcript.items.clear();
    small
        .transcript
        .items
        .push(incoming(5, "2026-10-02T14:00:00Z", "Continue"));
    reply(&small);

    let mut large = rounds(1, 90_000);
    large.reply.budget.context_tokens = 200_000;
    large.compaction.as_mut().unwrap().context_tokens = 200_000;
    large.previous = Some(reply(&large));
    large.reply.budget.context_tokens = 30_000;
    assert!(matches!(
        Plugin::compose("{}".into(), large).unwrap(),
        Composition::Compact(_)
    ));
}

#[test]
fn changing_image_selection_discards_the_measurement() {
    let mut input = rounds(1, 60);
    let picture = image(12);
    let item = &mut input.transcript.items[0];
    item.message.parts.push(Part::Image(picture.clone()));
    let EventBody::UserMessage(message) = &mut item.event.as_mut().unwrap().body else {
        panic!("message fixture")
    };
    message.attachments.push(AttachmentRef::Image(picture));
    input.previous = Some(reply(&input));
    input.previous_usage = usage(81_000);
    input.reply.accepts.clear();
    assert!(!reply(&input).omitted.is_empty());
}

#[test]
fn compact_at_scales_the_trigger_and_retained_tail() {
    let mut input = rounds(8, 6_000);
    input.reply.budget.context_tokens = 30_000;
    reply(&input);
    let Composition::Compact(compaction) =
        Plugin::compose(r#"{"compact_at":50}"#.into(), input).unwrap()
    else {
        panic!("expected compaction")
    };
    // Each Round is just over 2k tokens: keeping 18.75% of 30k retains two, not three.
    assert_eq!(compaction.upto, pos(23));
}

#[test]
fn owner_requested_compaction_chooses_the_latest_fitting_round_end() {
    for appended_input in [false, true] {
        let mut input = rounds(3, 3_000);
        input.compaction_requested = true;
        if appended_input {
            input
                .transcript
                .items
                .push(incoming(13, "2026-10-02T14:00:00Z", "Continue"));
        }
        let Composition::Compact(compaction) = Plugin::compose("{}".into(), input.clone()).unwrap()
        else {
            panic!("expected compaction")
        };
        assert_eq!(compaction.upto, pos(11));
        input.compaction.as_mut().unwrap().context_tokens = 3_000;
        let Composition::Compact(compaction) = Plugin::compose("{}".into(), input).unwrap() else {
            panic!("expected compaction")
        };
        assert_eq!(compaction.upto, pos(7));
    }
}

#[test]
fn an_unavailable_or_unusable_compaction_still_allows_the_reply() {
    let mut input = rounds(1, 60);
    input.compaction_requested = true;
    input.compaction = None;
    reply(&input);
    input.compaction = Some(Budget {
        context_tokens: 30_000,
        max_output_tokens: 512,
    });
    input.transcript.round_ends.clear();
    reply(&input);
    input.transcript.round_ends.push(pos(3));
    input.compactions_left = 0;
    reply(&input);
}
