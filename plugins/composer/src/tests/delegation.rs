use super::*;
use enco_sdk::types::{Effect, ToolSpec};

pub(super) fn capability(name: &str) -> Capability {
    Capability {
        id: CapabilityId {
            node: "node".into(),
            name: name.into(),
        },
        spec: ToolSpec {
            name: name.into(),
            description: name.into(),
            input_schema: "{}".into(),
            effect: Effect::ReadOnly,
        },
    }
}

/// `research`, a child of `main`.
pub(super) fn child_source() -> EventSource {
    EventSource::Child(SessionSource {
        id: "child".into(),
        name: "main/research".into(),
    })
}

fn child() -> Child {
    Child {
        name: "research".into(),
        profile: "researcher".into(),
        awaiting: true,
        halted: None,
        replies_to_owner: false,
    }
}

fn profile() -> DelegableProfile {
    DelegableProfile {
        name: "researcher".into(),
        description: "Find evidence".into(),
        reply: vec![],
    }
}

/// An input from `source`; `interactive` as the kernel decides it for that source and body.
fn session_input(
    seq: u64,
    source: EventSource,
    body: EventBody,
    interactive: bool,
) -> TranscriptItem {
    let mut item = incoming(seq, "2026-10-02T14:00:00Z", "input");
    let event = item.event.as_mut().unwrap();
    event.source = source;
    event.interactive = interactive;
    item.message.parts = match &body {
        EventBody::SessionMessage(message) => vec![Part::Text(message.text.clone())],
        _ => vec![],
    };
    event.body = body;
    item
}

fn own(seq: u64, role: Role, part: Part) -> TranscriptItem {
    TranscriptItem {
        pos: pos(seq),
        event: None,
        message: Message {
            role,
            parts: vec![part],
        },
    }
}

fn call(seq: u64) -> [TranscriptItem; 2] {
    [
        own(
            seq,
            Role::Assistant,
            Part::ToolCall(Call {
                id: format!("call-{seq}"),
                name: "shell".into(),
                arguments: "{}".into(),
            }),
        ),
        own(
            seq + 1,
            Role::Tool,
            Part::ToolResult(CallResult {
                call_id: format!("call-{seq}"),
                content: "done".into(),
                is_error: false,
            }),
        ),
    ]
}

/// The note immediately before the Log item at `seq`.
fn note_before(plan: &Plan, seq: u64) -> &str {
    let at = plan
        .items
        .iter()
        .position(|item| matches!(item, PlanItem::Log(reference) if reference.pos == pos(seq)))
        .unwrap();
    let PlanItem::Message(note) = &plan.items[at - 1] else {
        panic!("no note before {seq}")
    };
    &note.text
}

/// The source IDs of each status item from `from` on, with the item's index.
fn statuses(plan: &Plan, from: usize) -> Vec<(usize, Vec<&str>)> {
    plan.items
        .iter()
        .enumerate()
        .skip(from)
        .filter_map(|(index, item)| match item {
            PlanItem::Message(message)
                if message
                    .sources
                    .iter()
                    .any(|source| ["children", "profiles"].contains(&source.id.as_str())) =>
            {
                assert_eq!(message.role, Role::User);
                Some((
                    index,
                    message.sources.iter().map(|s| s.id.as_str()).collect(),
                ))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn delegated_messages_name_their_sessions_as_notes_and_summaries_do() {
    let mut input = input();
    input.session.name = "main/research".into();
    input.session.parent = Some(SessionSource {
        id: "parent".into(),
        name: "main".into(),
    });
    let task = EventBody::SessionMessage(SessionMessage {
        text: "Find evidence".into(),
        file: None,
    });
    let report = EventBody::SessionMessage(SessionMessage {
        text: "Sources found".into(),
        file: Some(FileRef {
            name: "report.md".into(),
            size: 42,
            content: "hash".into(),
            path: "/work/report.md".into(),
        }),
    });
    let child = EventSource::Child(SessionSource {
        id: "grandchild".into(),
        name: "main/research/sources".into(),
    });
    input.transcript.items = vec![
        session_input(1, EventSource::Parent, task, true),
        session_input(2, child, report, false),
    ];
    let plan = reply(&input);
    assert!(note_before(&plan, 1).contains("`main`"));
    let report = note_before(&plan, 2);
    assert!(report.contains("`sources`"));
    assert!(!report.contains("main/research/sources"));
    for data in ["report.md", "42", "/work/report.md"] {
        assert!(report.contains(data));
    }

    input.reply.budget.context_tokens = 5000;
    input.transcript.items[1].message.parts = vec![Part::Text("x".repeat(20_000))];
    input.transcript.round_ends.push(pos(3));
    let Composition::Compact(compaction) = Plugin::compose("{}".into(), input).unwrap() else {
        panic!("expected compaction")
    };
    let summarized = text(&compaction.plan);
    assert!(summarized.contains("\nmain: Find evidence\n"));
    assert!(summarized.contains("\nsources: xxx"));
}

#[test]
fn child_outcomes_carry_their_failure_and_contract_violations_fail() {
    let mut input = input();
    input.transcript.items = vec![session_input(
        1,
        child_source(),
        EventBody::ChildRunEnded(RunEnd::Failed(enco_sdk::protocol::failure(
            "provider", "timeout", false,
        ))),
        false,
    )];
    let outcome = reply(&input);
    assert!(note_before(&outcome, 1).contains("`research`"));
    assert!(note_before(&outcome, 1).contains("provider: timeout"));
    // A completed Run answers with its reply, and only a Session with a parent receives its tasks.
    for (source, body, interactive) in [
        (
            child_source(),
            EventBody::ChildRunEnded(RunEnd::Completed),
            false,
        ),
        (
            EventSource::Parent,
            EventBody::SessionMessage(SessionMessage {
                text: "Find evidence".into(),
                file: None,
            }),
            true,
        ),
    ] {
        input.transcript.items = vec![session_input(1, source, body, interactive)];
        let failure = Plugin::compose("{}".into(), input.clone()).unwrap_err();
        assert_eq!(failure.code, "compose.failed");
    }
}

#[test]
fn halts_mention_only_the_inputs_since_the_last_run_and_never_offer_silence() {
    let mut input = input();
    input.session.name = "ancestor/main".into();
    input.session.parent = Some(SessionSource {
        id: "ancestor".into(),
        name: "ancestor".into(),
    });
    input.capabilities.push(capability("reply_skip"));
    let halt = || {
        EventBody::Halt(Halt {
            root: "ancestor".into(),
            by: Some("ancestor".into()),
        })
    };
    let reminder = || {
        EventBody::Reminder(Reminder {
            schedule: "schedule".into(),
            due_at: datetime("2026-10-02T14:00:00Z"),
            skipped: 0,
            text: "check".into(),
        })
    };
    let mut halt_note = |items: Vec<TranscriptItem>, run_end: u64| {
        let at = items.last().unwrap().pos.seq;
        input.transcript.items = items;
        input.transcript.run_ends = vec![RunEnded {
            pos: pos(run_end),
            end: RunEnd::Completed,
        }];
        note_before(&reply(&input), at).to_owned()
    };
    let idle = halt_note(
        vec![
            incoming(1, "2026-10-02T14:00:00Z", "hello"),
            session_input(3, EventSource::Parent, halt(), false),
        ],
        2,
    );
    let earlier = halt_note(
        vec![
            incoming(1, "2026-10-02T14:00:00Z", "hello"),
            session_input(2, EventSource::Scheduler, reminder(), false),
            session_input(4, EventSource::Parent, halt(), false),
        ],
        3,
    );
    let superseded = halt_note(
        vec![
            incoming(1, "2026-10-02T14:00:00Z", "hello"),
            session_input(3, EventSource::Scheduler, reminder(), false),
            session_input(4, EventSource::Parent, halt(), false),
        ],
        2,
    );
    assert!(idle.contains("`ancestor`"));
    assert!(!idle.contains("reply_skip"));
    assert_eq!(idle, earlier);
    assert_ne!(idle, superseded);
}

#[test]
fn delegation_guidance_follows_session_send() {
    let mut input = input();
    let head = |input: &ComposeInput| {
        let plan = reply(input);
        let PlanItem::Message(head) = &plan.items[0] else {
            panic!("the request starts with its system message")
        };
        head.text.clone()
    };
    assert!(!head(&input).contains("session_send"));
    input.capabilities.push(capability("session_send"));
    assert!(head(&input).contains("session_send"));
}

#[test]
fn status_follows_the_agents_own_work_and_precedes_new_inputs() {
    let mut input = input();
    input.capabilities.push(capability("session_send"));
    input.children.push(child());
    input.profiles.push(profile());
    input
        .transcript
        .items
        .push(incoming(1, "2026-10-02T14:00:00Z", "start"));
    let first = reply(&input);
    // The status comes before the input and the note that belongs to it.
    assert_eq!(statuses(&first, 0), [(1, vec!["children", "profiles"])]);
    assert!(matches!(first.items.last(), Some(PlanItem::Log(input)) if input.pos == pos(1)));

    // Without new input, the changed status comes last, after the call and its result.
    input.previous = Some(first.clone());
    input.children[0].awaiting = false;
    input.children[0].replies_to_owner = true;
    input.transcript.items.extend(call(2));
    let settled = reply(&input);
    assert!(settled.items.starts_with(&first.items));
    let tail = &settled.items[first.items.len()..];
    assert!(matches!(
        tail,
        [PlanItem::Log(call), PlanItem::Log(result), PlanItem::Message(_)]
            if (call.pos, result.pos) == (pos(2), pos(3))
    ));
    assert_eq!(statuses(&settled, first.items.len())[0].1, ["children"]);

    // With new input, it comes after the agent's own reply and before the input.
    input.previous = Some(settled.clone());
    input.children[0].halted = Some(Halt {
        root: "main/research".into(),
        by: None,
    });
    input
        .transcript
        .items
        .push(own(4, Role::Assistant, Part::Text("Delegated".into())));
    input
        .transcript
        .items
        .push(incoming(5, "2026-10-02T14:00:00Z", "status?"));
    let answered = reply(&input);
    let tail = &answered.items[settled.items.len()..];
    assert!(matches!(
        tail,
        [PlanItem::Log(reply), PlanItem::Message(_), PlanItem::Log(next)]
            if (reply.pos, next.pos) == (pos(4), pos(5))
    ));

    // Unchanged facts are not written again.
    input.previous = Some(answered.clone());
    input
        .transcript
        .items
        .push(incoming(6, "2026-10-02T14:00:00Z", "again"));
    let unchanged = reply(&input);
    assert!(statuses(&unchanged, answered.items.len()).is_empty());

    // A summary starts a new prefix, which shows the latest facts again.
    input.previous = Some(unchanged);
    input.transcript.summary = Some("Earlier work".into());
    input.transcript.items.drain(..5);
    let compacted = reply(&input);
    assert_eq!(statuses(&compacted, 0), [(1, vec!["children", "profiles"])]);
    let (PlanItem::Message(before), PlanItem::Message(after)) = (
        &answered.items[settled.items.len() + 1],
        &compacted.items[1],
    ) else {
        panic!("status notes")
    };
    assert_eq!(before.sources[0], after.sources[0]);
}

#[test]
fn status_appears_when_this_session_can_delegate_or_has_children() {
    let mut input = input();
    input
        .transcript
        .items
        .push(incoming(1, "2026-10-02T14:00:00Z", "start"));
    // The status sources this composition appends after the reused prefix.
    let ids = |input: &ComposeInput| {
        let reused = input.previous.as_ref().map_or(0, |plan| plan.items.len());
        statuses(&reply(input), reused)
            .into_iter()
            .flat_map(|(_, ids)| ids.into_iter().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    assert!(ids(&input).is_empty());
    input.children.push(child());
    let with_children = reply(&input);
    assert_eq!(ids(&input), ["children"]);
    // A status shown earlier does not keep the note once neither condition holds.
    input.children.clear();
    input.previous = Some(with_children);
    input
        .transcript
        .items
        .push(incoming(2, "2026-10-02T14:00:00Z", "archived"));
    assert!(ids(&input).is_empty());
    input.previous = None;
    input.capabilities.push(capability("session_send"));
    assert_eq!(ids(&input), ["children"]);
    input.profiles.push(profile());
    assert_eq!(ids(&input), ["children", "profiles"]);
}

#[test]
fn a_status_after_the_last_item_counts_toward_the_window() {
    let mut input = input();
    input.reply.budget.context_tokens = 5000;
    input.compaction = None;
    input.capabilities.push(capability("session_send"));
    input.profiles.push(profile());
    input
        .transcript
        .items
        .push(incoming(1, "2026-10-02T14:00:00Z", "start"));
    input.previous = Some(reply(&input));
    input.transcript.items.extend(call(2));
    input.profiles[0].description = "details ".repeat(3000);
    assert_eq!(
        Plugin::compose("{}".into(), input).unwrap_err().code,
        "context.overflow"
    );
}
