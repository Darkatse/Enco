use super::delegation::{capability, child_source};
use super::*;

#[test]
fn only_noninteractive_inputs_offer_silence_when_the_tool_is_available() {
    let owner = incoming(1, "2026-10-02T14:03:00Z", "hello");
    let message = owner.event.as_ref().unwrap().body.clone();
    let session_message = EventBody::SessionMessage(SessionMessage {
        text: "Evidence".into(),
        file: None,
    });
    // Whether each input is interactive, as the kernel decides it.
    let cases = [
        (EventSource::Cli, message.clone(), true),
        (member(false), message, true),
        (EventSource::Cli, EventBody::Backtrack(Some(pos(1))), true),
        (EventSource::Parent, session_message.clone(), true),
        (
            EventSource::Scheduler,
            EventBody::Reminder(Reminder {
                schedule: "schedule".into(),
                due_at: datetime("2026-10-02T14:03:00Z"),
                skipped: 0,
                text: "routine check".into(),
            }),
            false,
        ),
        (
            EventSource::Registry,
            EventBody::GenerationRolledBack(Rollback {
                plugin: "fixture".into(),
                trial: 2,
                replacement: Some(1),
                failure: enco_sdk::protocol::failure("plugin.trap", "probe trapped", false),
                text: "rolled back".into(),
            }),
            false,
        ),
        (child_source(), session_message, false),
        (
            child_source(),
            EventBody::ChildRunEnded(RunEnd::Interrupted),
            false,
        ),
    ];
    for available in [false, true] {
        for (source, body, interactive) in &cases {
            let mut input = input();
            input.session.parent = Some(SessionSource {
                id: "parent".into(),
                name: "root".into(),
            });
            if available {
                input.capabilities.push(capability("reply_skip"));
            }
            let mut item = incoming(2, "2026-10-02T14:05:00Z", "input");
            let event = item.event.as_mut().unwrap();
            event.source = source.clone();
            event.body = body.clone();
            event.interactive = *interactive;
            item.message.parts = match body {
                EventBody::Reminder(reminder) => vec![Part::Text(reminder.text.clone())],
                EventBody::GenerationRolledBack(rollback) => {
                    vec![Part::Text(rollback.text.clone())]
                }
                EventBody::UserMessage(message) => vec![Part::Text(message.text.clone())],
                EventBody::SessionMessage(message) => vec![Part::Text(message.text.clone())],
                EventBody::Backtrack(_) | EventBody::ChildRunEnded(_) | EventBody::Halt(_) => {
                    vec![]
                }
            };
            input.transcript.items = vec![owner.clone(), item];
            let plan = reply(&input);
            assert_eq!(
                plan.tools.iter().any(|id| id.name == "reply_skip"),
                available
            );
            let note = plan
                .items
                .iter()
                .rev()
                .find_map(|item| match item {
                    PlanItem::Message(message) => Some(message.text.as_str()),
                    PlanItem::Log(_) => None,
                })
                .unwrap();
            assert_eq!(note.contains("reply_skip"), available && !interactive);
        }
    }
}
