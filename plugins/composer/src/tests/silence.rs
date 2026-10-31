use super::*;
use enco_sdk::types::{Effect, ToolSpec};

#[test]
fn only_noninteractive_inputs_offer_silence_when_the_tool_is_available() {
    let owner = incoming(1, "2026-10-02T14:03:00Z", "hello");
    let message = owner.event.as_ref().unwrap().body.clone();
    let cases = [
        (EventSource::Cli, message.clone(), false),
        (member(false), message, false),
        (EventSource::Cli, EventBody::Backtrack(pos(1)), false),
        (
            EventSource::Scheduler,
            EventBody::Reminder(Reminder {
                schedule: "schedule".into(),
                due_at: datetime("2026-10-02T14:03:00Z"),
                skipped: 0,
                text: "routine check".into(),
            }),
            true,
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
            true,
        ),
    ];
    for available in [false, true] {
        for (source, body, noninteractive) in &cases {
            let mut input = input();
            if available {
                input.capabilities.push(Capability {
                    id: CapabilityId {
                        node: "node".into(),
                        name: "reply_skip".into(),
                    },
                    spec: ToolSpec {
                        name: "reply_skip".into(),
                        description: "End without replying".into(),
                        input_schema:
                            r#"{"type":"object","properties":{},"additionalProperties":false}"#
                                .into(),
                        effect: Effect::ReadOnly,
                    },
                });
            }
            let mut item = incoming(2, "2026-10-02T14:05:00Z", "input");
            let event = item.event.as_mut().unwrap();
            event.source = source.clone();
            event.body = body.clone();
            item.message.parts = match body {
                EventBody::Reminder(reminder) => vec![Part::Text(reminder.text.clone())],
                EventBody::GenerationRolledBack(rollback) => {
                    vec![Part::Text(rollback.text.clone())]
                }
                EventBody::Backtrack(_) => vec![],
                EventBody::UserMessage(message) => vec![Part::Text(message.text.clone())],
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
            assert_eq!(note.contains("reply_skip"), available && *noninteractive);
        }
    }
}
