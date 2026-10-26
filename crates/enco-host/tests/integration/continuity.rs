#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;

use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::*;
use tokio::sync::broadcast;

struct TestClock(Mutex<DateTime<Utc>>);

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

impl TestClock {
    fn advance(&self, seconds: i64) {
        *self.0.lock().unwrap() += chrono::Duration::seconds(seconds);
    }
}

#[tokio::test(start_paused = true)]
async fn a_one_shot_reminder_in_the_past_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(Utc::now())));
    let (kernel, _) = kernel_with(dir.path(), ScriptedProvider::new(vec![]), Tz::UTC, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    assert!(matches!(
        kernel
            .schedules()
            .create(
                session.id,
                ScheduleRule::Once {
                    at: clock.now() - chrono::Duration::seconds(1)
                },
                "past".into()
            )
            .await,
        Err(ScheduleError::NoOccurrenceAfter(_))
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn reminders_fire_once_and_overdue_reminders_resume_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(Utc::now())));
    let provider = ScriptedProvider::new(vec![reply("first reminder"), reply("overdue reminder")]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), Tz::UTC, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    let first = kernel
        .schedules()
        .create(
            session.id,
            ScheduleRule::Once {
                at: clock.now() + chrono::Duration::seconds(10),
            },
            "first".into(),
        )
        .await
        .unwrap();
    clock.advance(11);
    tokio::time::advance(Duration::from_secs(1)).await;
    let entries = finish(&mut rx).await;
    assert_eq!(
        entries
            .iter()
            .filter(|entry| is_reminder(entry, first.schedule.id))
            .count(),
        1
    );
    // A delivered one-shot reminder is done and no longer listed.
    assert!(kernel.schedules().list(None).await.unwrap().is_empty());
    let cancelled = kernel
        .schedules()
        .create(
            session.id,
            ScheduleRule::Once {
                at: clock.now() + chrono::Duration::seconds(10),
            },
            "cancelled".into(),
        )
        .await
        .unwrap();
    assert!(
        kernel
            .schedules()
            .cancel(cancelled.schedule.id)
            .await
            .unwrap()
    );
    assert!(
        !kernel
            .schedules()
            .cancel(cancelled.schedule.id)
            .await
            .unwrap()
    );
    let overdue = kernel
        .schedules()
        .create(
            session.id,
            ScheduleRule::Once {
                at: clock.now() + chrono::Duration::seconds(20),
            },
            "overdue".into(),
        )
        .await
        .unwrap();
    kernel.shutdown().await.unwrap();
    drop(kernel);
    clock.advance(30);
    let (kernel, _) = kernel_with(dir.path(), provider, Tz::UTC, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let mut rx = kernel.subscribe(session.id).unwrap();
    let log = kernel.log(session.id, None).await.unwrap();
    if !log
        .iter()
        .any(|entry| is_reminder(entry, overdue.schedule.id))
    {
        finish(&mut rx).await;
    }
    kernel.shutdown().await.unwrap();
    let log = kernel.log(session.id, None).await.unwrap();
    for id in [first.schedule.id, overdue.schedule.id] {
        assert_eq!(log.iter().filter(|entry| is_reminder(entry, id)).count(), 1);
    }
    assert!(
        !log.iter()
            .any(|entry| is_reminder(entry, cancelled.schedule.id))
    );
}

#[tokio::test]
async fn compacted_requests_are_recorded_and_the_summary_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("fs_list", r#"{"path":"."}"#)]),
        reply("scripted summary"),
        reply("finished"),
    ]);
    let (kernel, _) = kernel_configured_composer(
        dir.path(),
        provider.clone(),
        configuration(),
        vec![],
        Arc::new(HistoryComposer),
        |_| {},
    )
    .await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    kernel
        .submit(session, EventId::new(), "initial request".into(), vec![])
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let requests = provider.requests.lock().unwrap().clone();
    let attempts: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted { attempt, .. } => Some(attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts.len(), 3);
    for (attempt, request) in attempts.into_iter().zip(requests) {
        let inspected = kernel.inspect(session, Some(attempt)).await.unwrap();
        assert_eq!(
            serde_json::to_value(inspected.request).unwrap(),
            serde_json::to_value(request).unwrap()
        );
    }
    assert!(entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::Compacted { summary, .. } if summary == "scripted summary")));
    kernel.shutdown().await.unwrap();
    drop(kernel);
    let provider = ScriptedProvider::new(vec![reply("continued")]);
    let (kernel, _) = support::kernel(dir.path(), provider.clone()).await;
    let mut rx = kernel.subscribe(session).unwrap();
    kernel
        .submit(session, EventId::new(), "continue".into(), vec![])
        .await
        .unwrap();
    finish(&mut rx).await;
    let request = provider.requests.lock().unwrap()[0].clone();
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.joined_text() == "scripted summary")
    );
    assert!(
        !request
            .messages
            .iter()
            .any(|message| message.joined_text() == "initial request")
    );
    kernel.shutdown().await.unwrap();
}

async fn new_york(root: &Path, provider: Arc<ScriptedProvider>, clock: &Arc<TestClock>) -> Kernel {
    let clock = clock.clone();
    kernel_with(root, provider, Tz::America__New_York, |deps| {
        deps.clock = clock;
    })
    .await
    .0
}

fn utc(at: &str) -> DateTime<Utc> {
    at.parse().unwrap()
}

/// Creates a daily 08:00 briefing in the owner's New York zone and delivers its first
/// occurrence; returns the briefing as created.
async fn deliver_first_briefing(
    root: &Path,
    provider: Arc<ScriptedProvider>,
    clock: &Arc<TestClock>,
) -> (Kernel, SessionId, Scheduled) {
    let kernel = new_york(root, provider, clock).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    let briefing = kernel
        .schedules()
        .create(
            session,
            ScheduleRule::Cron {
                expr: "0 8 * * *".into(),
                timezone: None,
            },
            "daily briefing".into(),
        )
        .await
        .unwrap();
    *clock.0.lock().unwrap() = briefing.next_due;
    tokio::time::advance(Duration::from_secs(1)).await;
    finish(&mut rx).await;
    (kernel, session, briefing)
}

/// Stops the node over three missed local mornings, including the DST change, and restarts it
/// on 2026-03-10 once the catch-up Run has ended.
async fn restart_after_missed_mornings(
    root: &Path,
    provider: Arc<ScriptedProvider>,
    clock: &Arc<TestClock>,
    kernel: Kernel,
    session: SessionId,
) -> (Kernel, broadcast::Receiver<Entry>) {
    kernel.shutdown().await.unwrap();
    drop(kernel);
    *clock.0.lock().unwrap() = utc("2026-03-10T12:01:00Z");
    let kernel = new_york(root, provider, clock).await;
    let mut rx = kernel.subscribe(session).unwrap();
    finish(&mut rx).await;
    (kernel, rx)
}

#[tokio::test(start_paused = true)]
async fn recurring_schedules_resume_from_their_last_delivery_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(utc("2026-03-06T13:01:00Z"))));
    let provider = ScriptedProvider::new((0..3).map(|_| reply("reminded")).collect());
    let (kernel, session, briefing) =
        deliver_first_briefing(dir.path(), provider.clone(), &clock).await;
    assert_eq!(briefing.next_due, utc("2026-03-07T13:00:00Z"));
    let next = kernel.schedules().list(None).await.unwrap().remove(0);
    assert_eq!(next.next_due, utc("2026-03-08T12:00:00Z"));
    let (kernel, _) =
        restart_after_missed_mornings(dir.path(), provider, &clock, kernel, session).await;
    let log = kernel.log(session, None).await.unwrap();
    let reminders: Vec<_> = log
        .iter()
        .filter(|entry| is_reminder(entry, briefing.schedule.id))
        .collect();
    assert_eq!(reminders.len(), 2);
    // The three missed mornings become one input.
    assert!(
        matches!(&reminders[1].body, EntryBody::EventConsumed { event: Event { body: EventBody::Reminder { skipped: 2, due_at, .. }, .. } }
        if *due_at == utc("2026-03-10T12:00:00Z"))
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn owner_zone_changes_move_only_floating_schedules() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(utc("2026-03-06T13:01:00Z"))));
    let provider = ScriptedProvider::new((0..3).map(|_| reply("reminded")).collect());
    let (kernel, session, briefing) =
        deliver_first_briefing(dir.path(), provider.clone(), &clock).await;
    let (kernel, mut rx) =
        restart_after_missed_mornings(dir.path(), provider, &clock, kernel, session).await;
    let inspection = kernel.inspect(session, None).await.unwrap();
    let fixed = kernel
        .schedules()
        .create(
            session,
            ScheduleRule::Cron {
                expr: "0 8 * * *".into(),
                timezone: Some(Tz::America__New_York),
            },
            "fixed zone".into(),
        )
        .await
        .unwrap();
    // A live westward change uses the same catch-up rule; an explicit zone stays fixed.
    *clock.0.lock().unwrap() = utc("2026-03-10T16:00:00Z");
    let mut document = kernel.registry().view().config.document.clone();
    document["owner"]["timezone"] = json!(Tz::America__Los_Angeles);
    submit(kernel.registry(), configuration_request(document))
        .await
        .unwrap();
    finish(&mut rx).await;
    assert_eq!(
        kernel
            .inspect(session, Some(inspection.attempt))
            .await
            .unwrap()
            .request,
        inspection.request
    );
    let active = kernel.schedules().list(None).await.unwrap();
    let following = active
        .iter()
        .find(|s| s.schedule.id == briefing.schedule.id)
        .unwrap();
    assert_eq!(following.next_due, utc("2026-03-11T15:00:00Z"));
    assert_eq!(
        active
            .iter()
            .find(|s| s.schedule.id == fixed.schedule.id)
            .unwrap()
            .next_due,
        fixed.next_due
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_schedule_arguments_leave_no_records_and_allow_correction() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(
        "2026-10-09T12:00:00Z".parse().unwrap(),
    )));
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, store) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        |deps| {
            deps.clock = clock;
        },
    )
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    let cases = [
        json!({ "cron": "invalid" }),
        json!({ "cron": "0 8 * * *", "timezone": "Unknown/Zone" }),
        json!({ "cron": "0 0 30 2 *" }),
        json!({ "cron": "* 8 * * *" }),
        json!({ "cron": "0 8 * * *", "at": "2026-10-10T08:00:00-04:00" }),
        json!({}),
        json!({ "at": "2026-10-10T08:00:00-04:00", "timezone": "America/New_York" }),
    ];
    for mut args in cases {
        args["message"] = json!("briefing");
        provider.steps.lock().unwrap().extend([
            calls(vec![call("schedule_create", &args.to_string())]),
            reply("please correct the rule"),
        ]);
        kernel
            .submit(
                session.id,
                EventId::new(),
                "set a reminder".into(),
                Vec::new(),
            )
            .await
            .unwrap();
        let entries = finish(&mut rx).await;
        assert!(
            entries.iter().any(|entry| matches!(
                &entry.body,
                EntryBody::ToolCallSettled { outcome: Settlement::Failed { failure }, .. }
                    if failure.code == code::TOOL_INVALID_ARGUMENTS
            )),
            "{args}"
        );
        assert!(store.schedules(None).await.unwrap().is_empty(), "{args}");
    }
    provider.steps.lock().unwrap().extend([
        calls(vec![call(
            "schedule_create",
            r#"{"cron":"0 8 * * *","message":"briefing"}"#,
        )]),
        reply("scheduled"),
    ]);
    kernel
        .submit(
            session.id,
            EventId::new(),
            "use eight o'clock".into(),
            Vec::new(),
        )
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert!(entries.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            ..
        }
    )));
    assert_eq!(store.schedules(None).await.unwrap().len(), 1);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn stopped_scheduler_reports_the_schedule_that_failed() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(store_paths(dir.path())).await.unwrap();
    let session = store.ensure_session("main", Utc::now()).await.unwrap();
    let schedule = Schedule {
        id: ScheduleId::new(),
        session: session.id,
        rule: ScheduleRule::Cron {
            expr: "invalid".into(),
            timezone: None,
        },
        message: "briefing".into(),
        created_at: session.created_at,
        state: ScheduleState::Active,
        last: None,
    };
    store.insert_schedule(&schedule).await.unwrap();
    drop(store);
    let (kernel, _) = kernel(dir.path(), ScriptedProvider::new(vec![])).await;
    let failure = kernel.schedules().list(None).await.unwrap_err();
    let stopped = kernel.status().await.unwrap().scheduler_stopped.unwrap();
    assert!(stopped.contains(&schedule.id.to_string()));
    assert!(matches!(failure, ScheduleError::Stopped(Some(reason)) if reason == stopped));
    kernel.shutdown().await.unwrap();
}
