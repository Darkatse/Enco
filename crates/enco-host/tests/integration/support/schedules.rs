//! A schedule exists only as its Session's own facts, so a test creates one the way an Agent does
//! and delivers reminders the way the Scheduler does.
use super::{call, calls, finish, reply};
use enco_core::*;
use enco_kernel::*;
use tokio::sync::broadcast;

/// A call creating a schedule that is due only on 29 February, so its reminders reach the
/// Session only when a test delivers them.
pub fn schedule_creation() -> ToolCall {
    call(
        "schedule_create",
        r#"{"cron":"0 0 29 2 *","message":"routine check"}"#,
    )
}

/// A model step making only `schedule_creation`.
pub fn create_schedule() -> Result<Completion, Failure> {
    calls(vec![schedule_creation()])
}

/// The model's answers when `agent_schedule` asks it for a schedule.
pub fn scheduling() -> [Result<Completion, Failure>; 2] {
    [create_schedule(), reply("scheduled")]
}

/// The schedule created in `entries`, as its settlement records it.
pub fn created(entries: &[Entry]) -> ScheduleId {
    entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                schedule: Some(ScheduleChange::Created { id, .. }),
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("no schedule was created")
}

/// Ask the Agent of `session` for a schedule; its script answers with `scheduling`.
pub async fn agent_schedule(
    kernel: &Kernel,
    session: SessionId,
    rx: &mut broadcast::Receiver<Entry>,
) -> ScheduleId {
    kernel
        .submit(session, EventId::new(), "remind me".into(), Vec::new())
        .await
        .unwrap();
    created(&finish(rx).await)
}

/// Deliver a reminder of `schedule` that is due now, as the Scheduler does.
pub async fn remind(kernel: &Kernel, session: SessionId, schedule: ScheduleId) {
    let due_at = Utc::now();
    let accepted = kernel
        .accept(
            &[Event {
                id: EventId::reminder(schedule, due_at),
                session,
                source: EventSource::Scheduler,
                body: EventBody::Reminder {
                    schedule,
                    due_at,
                    skipped: 0,
                    text: "routine check".into(),
                },
                received_at: due_at,
            }],
            None,
        )
        .await
        .unwrap();
    assert_eq!(accepted, [Accepted::New]);
}
