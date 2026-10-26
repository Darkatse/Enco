use super::*;
use crate::composer::*;
use enco_sdk::types::{ImageFormat, Role};

fn pos(seq: u64) -> LogPos {
    LogPos { epoch: 1, seq }
}

fn datetime(text: &str) -> Datetime {
    let value = chrono::DateTime::parse_from_rfc3339(text).unwrap();
    Datetime {
        seconds: u64::try_from(value.timestamp()).unwrap(),
        nanoseconds: value.timestamp_subsec_nanos(),
    }
}

fn memory(id: &str, text: String, pinned: bool) -> Memory {
    Memory {
        id: id.into(),
        text,
        pinned,
        created_at: datetime("2026-10-01T00:00:00Z"),
        updated_at: datetime("2026-10-01T00:00:00Z"),
        rev: 1,
    }
}

fn input() -> ComposeInput {
    ComposeInput {
        session: Session {
            id: "session".into(),
            name: "main".into(),
            profile: "default".into(),
            created_at: datetime("2026-10-01T00:00:00Z"),
        },
        timezone: "America/New_York".into(),
        home: "/home/enco".into(),
        transcript: Transcript {
            summary: None,
            items: vec![],
            round_ends: vec![],
            run_ends: vec![],
        },
        previous: None,
        instructions: None,
        memories: vec![],
        omitted: vec![],
        capabilities: vec![],
        lifeline: vec![],
        unavailable: vec![],
        safe_mode: None,
        reply: ReplyTarget {
            budget: Budget {
                context_tokens: 100_000,
                max_output_tokens: 512,
            },
            accepts: vec![Modality::Image],
            skipped: vec![],
        },
        compaction: Some(Budget {
            context_tokens: 30_000,
            max_output_tokens: 512,
        }),
        compactions_left: 2,
    }
}

fn incoming(seq: u64, at: &str, text: &str) -> TranscriptItem {
    TranscriptItem {
        pos: pos(seq),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text(text.into())],
        },
        event: Some(Event {
            id: format!("event-{seq}"),
            received_at: datetime(at),
            source: EventSource::Cli,
            body: EventBody::UserMessage(UserMessage {
                text: text.into(),
                images: vec![],
                unviewable: vec![],
            }),
        }),
    }
}

fn reply(input: &ComposeInput) -> Plan {
    let Composition::Reply(plan) = compose::compose(input).unwrap() else {
        panic!("expected reply")
    };
    plan
}

fn text(plan: &Plan) -> String {
    plan.items
        .iter()
        .filter_map(|item| match item {
            PlanItem::Message(message) => Some(message.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn requests_keep_a_stable_prefix_and_date_inputs_across_dst() {
    let mut input = input();
    let cases = [
        ("2026-11-01T00:00:00-04:00", Some("2026-11-01 00:00")),
        ("2026-11-01T00:59:00-04:00", None),
        ("2026-11-01T01:01:00-04:00", Some("2026-11-01 01:01")),
        ("2026-11-01T01:20:00-05:00", Some("2026-11-01 01:20")),
        ("2026-11-01T03:00:00-05:00", Some("2026-11-01 03:00")),
    ];
    for (index, (at, expected)) in cases.into_iter().enumerate() {
        let before = input
            .previous
            .as_ref()
            .map(|plan| plan.items.len())
            .unwrap_or(1);
        input
            .transcript
            .items
            .push(incoming(u64::try_from(index + 1).unwrap(), at, "also"));
        let plan = reply(&input);
        if let Some(previous) = &input.previous {
            assert!(plan.items.starts_with(&previous.items));
        }
        let appended = Plan {
            items: plan.items[before..].to_vec(),
            ..plan.clone()
        };
        if let Some(expected) = expected {
            assert!(text(&appended).contains(expected));
        } else {
            assert_eq!(appended.items.len(), 1);
        }
        input.previous = Some(plan);
    }
}

#[test]
fn recalled_memories_keep_their_original_note_and_changed_sources_appear_again() {
    let mut input = input();
    input.memories = vec![
        memory("river", "Owner is River".into(), true),
        memory("food", "Likes rice".into(), false),
    ];
    input
        .transcript
        .items
        .push(incoming(1, "2026-10-02T14:00:00Z", "hello"));
    let first = reply(&input);
    let PlanItem::Message(head) = &first.items[0] else {
        panic!("the request starts with its system message");
    };
    assert!(head.text.contains("Owner is River"));
    assert!(
        head.sources
            .iter()
            .any(|source| source.id == "memory:river")
    );
    assert_eq!(text(&first).matches("Likes rice").count(), 1);
    input.previous = Some(first.clone());
    input
        .transcript
        .items
        .push(incoming(2, "2026-10-02T14:01:00Z", "again"));
    let second = reply(&input);
    assert!(second.items.starts_with(&first.items));
    assert_eq!(text(&second).matches("Likes rice").count(), 1);
    input.previous = Some(second.clone());
    input.memories[1].text = "Likes noodles".into();
    input
        .transcript
        .items
        .push(incoming(3, "2026-10-02T14:02:00Z", "changed"));
    let third = reply(&input);
    assert!(third.items.starts_with(&second.items));
    assert!(text(&third).contains("Likes noodles"));
}

#[test]
fn changed_instructions_and_compacted_history_start_a_new_prefix() {
    let mut input = input();
    input.instructions = Some("Use short answers".into());
    input
        .transcript
        .items
        .push(incoming(1, "2026-10-02T14:00:00Z", "hello"));
    let first = reply(&input);
    input.previous = Some(first.clone());
    input.instructions = Some("Explain in detail".into());
    let changed = reply(&input);
    assert!(!changed.items.starts_with(&first.items));
    assert!(text(&changed).contains("Explain in detail"));
    assert!(!text(&changed).contains("Use short answers"));
    input.previous = Some(changed);
    input.transcript.summary = Some("We discussed preferences".into());
    input.transcript.items = vec![incoming(10, "2026-10-03T14:00:00Z", "continue")];
    let resumed = reply(&input);
    assert!(text(&resumed).contains("We discussed preferences"));
    assert!(
        !resumed
            .items
            .iter()
            .any(|item| matches!(item, PlanItem::Log(reference) if reference.pos == pos(1)))
    );
}

#[test]
fn instructions_and_memories_exceeding_their_share_are_omitted_with_sources() {
    let mut input = input();
    input.instructions = Some("x".repeat(40_000));
    input
        .memories
        .push(memory("large", "y".repeat(50_000), true));
    let plan = reply(&input);
    assert!(
        plan.omitted
            .iter()
            .any(|omitted| omitted.source == "instructions:AGENTS.md")
    );
    assert!(
        plan.omitted
            .iter()
            .any(|omitted| omitted.source == "memory:large")
    );
}

#[test]
fn compaction_only_round_after_a_summary_cannot_produce_an_empty_summary_request() {
    let mut input = input();
    input.reply.budget.context_tokens = 5000;
    input.transcript.summary = Some("Earlier conversation".into());
    input.transcript.items.push(TranscriptItem {
        pos: pos(10),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text("x".repeat(20_000))],
        },
        event: None,
    });
    input.transcript.round_ends = vec![pos(5)];
    assert_eq!(
        compose::compose(&input).unwrap_err().code,
        "context.overflow"
    );
    input.transcript.round_ends.push(pos(20));
    assert!(
        matches!(compose::compose(&input).unwrap(), Composition::Compact(compaction) if compaction.upto == pos(20))
    );
}

#[test]
fn compaction_carries_circumstances_and_attachments_without_pinned_memory_or_tools() {
    let mut input = input();
    input.reply.budget.context_tokens = 5000;
    input
        .memories
        .push(memory("owner", "Owner is River".into(), true));
    input.transcript.run_ends.push(RunEnded {
        pos: pos(2),
        end: RunEnd::Failed(enco_sdk::protocol::failure(
            "provider.bad_request",
            "request rejected",
            false,
        )),
    });
    input.transcript.items.push(incoming(
        3,
        "2026-10-02T14:03:00Z",
        &"context ".repeat(2500),
    ));
    let EventBody::UserMessage(message) =
        &mut input.transcript.items[0].event.as_mut().unwrap().body
    else {
        panic!("owner fixture")
    };
    message.unviewable.push(Unviewable {
        what: "voice message".into(),
        reason: UnviewableReason::Unsupported,
    });
    input.transcript.round_ends.push(pos(4));
    let Composition::Compact(compaction) = compose::compose(&input).unwrap() else {
        panic!("expected compaction")
    };
    let text = text(&compaction.plan);
    assert!(text.contains("2026-10-02 10:03"));
    assert!(text.contains("provider.bad_request: request rejected"));
    assert!(text.contains("voice message: unsupported"));
    assert!(!text.contains("Owner is River"));
    assert!(compaction.plan.tools.is_empty());
}

fn image(size: u64) -> ImageRef {
    ImageRef {
        format: ImageFormat::Png,
        size,
        content: format!("hash-{size}"),
        path: format!("/images/{size}.png"),
    }
}

#[test]
fn images_stop_at_the_first_entry_that_does_not_fit_and_text_targets_keep_descriptions() {
    let mut input = input();
    for (index, (count, megabytes)) in [(1, 4), (2, 9), (2, 9), (1, 9)].into_iter().enumerate() {
        let image = image(megabytes * 1024 * 1024);
        let mut item = incoming(
            u64::try_from(index + 1).unwrap(),
            "2026-10-02T14:00:00Z",
            "describe",
        );
        item.message
            .parts
            .extend(vec![Part::Image(image.clone()); count]);
        let EventBody::UserMessage(message) = &mut item.event.as_mut().unwrap().body else {
            panic!("owner fixture")
        };
        message.images = vec![image; count];
        input.transcript.items.push(item);
    }
    let plan = reply(&input);
    let selected: Vec<_> = plan
        .items
        .iter()
        .filter_map(|item| match item {
            PlanItem::Log(reference) => Some(reference.images),
            _ => None,
        })
        .collect();
    assert_eq!(selected, [false, false, true, true]);
    assert_eq!(plan.omitted.len(), 3);
    input.previous = Some(plan);
    input.reply.accepts.clear();
    let text_only = reply(&input);
    assert!(
        text_only
            .items
            .iter()
            .all(|item| !matches!(item, PlanItem::Log(reference) if reference.images))
    );
    assert!(text(&text_only).contains("file: /images/9437184.png"));
    assert_eq!(text_only.omitted.len(), 6);
}

#[test]
fn an_oversized_image_entry_is_omitted_as_a_whole() {
    let mut input = input();
    let mut item = incoming(1, "2026-10-02T14:00:00Z", "images");
    item.message
        .parts
        .extend(vec![Part::Image(image(9 * 1024 * 1024)); 4]);
    input.transcript.items.push(item);
    let plan = reply(&input);
    assert!(
        plan.items
            .iter()
            .all(|item| !matches!(item, PlanItem::Log(reference) if reference.images))
    );
    assert_eq!(plan.omitted.len(), 4);
}

#[test]
fn reminders_name_their_due_time_and_missed_occurrences() {
    let mut input = input();
    let mut item = incoming(1, "2026-10-02T14:03:00Z", "briefing");
    item.event.as_mut().unwrap().body = EventBody::Reminder(Reminder {
        schedule: "schedule".into(),
        due_at: datetime("2026-10-01T12:00:00Z"),
        skipped: 2,
        text: "briefing".into(),
    });
    input.transcript.items.push(item);
    let plan = reply(&input);
    assert!(text(&plan).contains("reminder you scheduled for 2026-10-01 08:00"));
    assert!(text(&plan).contains("Missed earlier occurrences: 2"));
}

#[test]
fn a_small_compaction_window_allows_replies_until_the_reply_itself_overflows() {
    let mut input = input();
    input.reply.budget.context_tokens = 6000;
    input.reply.budget.max_output_tokens = 256;
    input.compaction = Some(Budget {
        context_tokens: 100,
        max_output_tokens: 10,
    });
    let mut replies = 0;
    let mut overflow = None;
    for seq in 1..=10 {
        input.transcript.items.push(incoming(
            seq * 10,
            "2026-10-02T14:00:00Z",
            &"some detailed history ".repeat(150),
        ));
        input.transcript.round_ends.push(pos(seq * 10 + 9));
        match compose::compose(&input) {
            Ok(Composition::Reply(plan)) => {
                replies += 1;
                input.previous = Some(plan);
            }
            Ok(Composition::Compact(_)) => panic!("no history boundary fits the compaction window"),
            Err(failure) => {
                overflow = Some(failure);
                break;
            }
        }
    }
    assert!(replies > 0);
    let failure = overflow.expect("history must outgrow the reply window");
    assert_eq!(failure.code, "context.overflow");
}
