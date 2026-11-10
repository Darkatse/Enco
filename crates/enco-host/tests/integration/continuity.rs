#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;

use enco_core::*;
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

/// Ask the Agent of `session` for a schedule with `arguments`; returns it as the owner lists it.
async fn schedule(
    kernel: &Kernel,
    provider: &ScriptedProvider,
    rx: &mut broadcast::Receiver<Entry>,
    session: SessionId,
    arguments: serde_json::Value,
) -> Scheduled {
    provider.script([
        calls(vec![call("schedule_create", &arguments.to_string())]),
        reply("scheduled"),
    ]);
    kernel
        .submit(session, EventId::new(), "remind me".into(), Vec::new())
        .await
        .unwrap();
    let id = created(&finish(rx).await);
    kernel
        .schedules(Some(session))
        .await
        .unwrap()
        .into_iter()
        .find(|scheduled| scheduled.schedule.id == id)
        .unwrap()
}

/// A one-time schedule `seconds` after the clock's present.
fn once(clock: &TestClock, seconds: i64, message: &str) -> serde_json::Value {
    json!({
        "at": (clock.now() + chrono::Duration::seconds(seconds)).to_rfc3339(),
        "message": message,
    })
}

#[tokio::test(start_paused = true)]
async fn reminders_fire_once_and_overdue_reminders_resume_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(Utc::now())));
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), Tz::UTC, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).await.unwrap();
    let first = schedule(
        &kernel,
        &provider,
        &mut rx,
        session.id,
        once(&clock, 10, "first"),
    )
    .await;
    provider.script([reply("first reminder")]);
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
    assert!(kernel.schedules(None).await.unwrap().is_empty());
    let cancelled = schedule(
        &kernel,
        &provider,
        &mut rx,
        session.id,
        once(&clock, 10, "cancelled"),
    )
    .await;
    assert!(kernel.cancel_schedule(cancelled.schedule.id).await.unwrap());
    assert!(!kernel.cancel_schedule(cancelled.schedule.id).await.unwrap());
    let overdue = schedule(
        &kernel,
        &provider,
        &mut rx,
        session.id,
        once(&clock, 20, "overdue"),
    )
    .await;
    provider.script([reply("overdue reminder")]);
    kernel.shutdown().await.unwrap();
    drop(kernel);
    clock.advance(30);
    let (kernel, _) = kernel_with(dir.path(), provider, Tz::UTC, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let mut rx = kernel.subscribe(session.id).await.unwrap();
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

/// Each settlement in `entries`: whether it succeeded, and the schedule change it records.
fn settlements(entries: &[Entry]) -> Vec<(bool, Option<ScheduleChange>)> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome, schedule, ..
            } => Some((matches!(outcome, Settlement::Ok), schedule.clone())),
            _ => None,
        })
        .collect()
}

/// The schedules each `schedule_list` call in `entries` answered with.
fn listed(entries: &[Entry]) -> Vec<Vec<ScheduleId>> {
    let lists: Vec<CallId> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallStarted {
                call, capability, ..
            } if capability.name == "schedule_list" => Some(*call),
            _ => None,
        })
        .collect();
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled { call, content, .. } if lists.contains(call) => {
                let listed: Vec<serde_json::Value> = serde_json::from_str(content).unwrap();
                Some(
                    listed
                        .iter()
                        .map(|scheduled| serde_json::from_value(scheduled["id"].clone()).unwrap())
                        .collect(),
                )
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_agent_answers_from_its_own_active_schedules_in_call_order() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = ScriptedProvider::new(vec![]);
    let provider = Arc::new(PausedProvider::new(scripted.clone(), None, false));
    let (kernel, store) = kernel(dir.path(), provider.clone()).await;
    let main = kernel.open_session("main").await.unwrap().id;
    let other = kernel.open_session("other").await.unwrap().id;
    let mut main_rx = kernel.subscribe(main).await.unwrap();
    let mut other_rx = kernel.subscribe(other).await.unwrap();
    let daily = json!({ "cron": "0 8 * * *", "message": "briefing" });
    let id = schedule(&kernel, &scripted, &mut main_rx, main, daily)
        .await
        .schedule
        .id;
    let cancel = |id: ScheduleId| call("schedule_cancel", &json!({ "id": id }).to_string());
    let list = || call("schedule_list", "{}");
    scripted.script([calls(vec![cancel(id)]), reply("not mine")]);
    kernel
        .submit(other, EventId::new(), "cancel it".into(), Vec::new())
        .await
        .unwrap();
    assert_eq!(settlements(&finish(&mut other_rx).await), [(false, None)]);
    assert_eq!(kernel.schedules(Some(main)).await.unwrap().len(), 1);
    // Each call of one reply sees what the calls before it settled.
    scripted.script([
        calls(vec![list(), cancel(id), cancel(id), list()]),
        reply("cancelled"),
    ]);
    kernel
        .submit(main, EventId::new(), "cancel it".into(), Vec::new())
        .await
        .unwrap();
    let entries = finish(&mut main_rx).await;
    assert_eq!(
        settlements(&entries),
        [
            (true, None),
            (true, Some(ScheduleChange::Cancelled { id })),
            (false, None),
            (true, None),
        ]
    );
    assert_eq!(listed(&entries), [vec![id], vec![]]);
    assert_eq!(
        store.schedules(Some(main)).await.unwrap()[0].state,
        ScheduleState::Cancelled
    );
    assert!(kernel.schedules(None).await.unwrap().is_empty());
    // The owner's cancel counts from its acceptance, before the Session consumes it.
    let weekly = json!({ "cron": "0 8 * * 1", "message": "review" });
    let weekly = schedule(&kernel, &scripted, &mut main_rx, main, weekly)
        .await
        .schedule
        .id;
    scripted.script([calls(vec![list(), cancel(weekly)]), reply("too late")]);
    provider
        .pause
        .store(true, std::sync::atomic::Ordering::SeqCst);
    kernel
        .submit(main, EventId::new(), "cancel weekly".into(), Vec::new())
        .await
        .unwrap();
    provider.entered.notified().await;
    assert!(kernel.cancel_schedule(weekly).await.unwrap());
    assert!(!kernel.cancel_schedule(weekly).await.unwrap());
    provider.resume.notify_one();
    let entries = finish(&mut main_rx).await;
    assert_eq!(settlements(&entries), [(true, None), (false, None)]);
    assert_eq!(listed(&entries), [vec![]]);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_stopped_session_takes_the_owners_cancel_but_refuses_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path()).await;
    let session = store
        .ensure_session(&"broken".parse().unwrap(), Utc::now())
        .await
        .unwrap()
        .id;
    let schedule = crate::store::Actor {
        store: &store,
        session,
    }
    .create_schedule(ScheduleRule::Cron {
        expr: "0 8 * * *".into(),
        timezone: None,
    })
    .await;
    // A future Log body is valid JSON but unreadable, so this Session's actor stops on start.
    rusqlite::Connection::open(dir.path().join("enco.db"))
        .unwrap()
        .execute(
            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,1,2,?,?)",
            rusqlite::params![
                session.to_string(),
                Utc::now().to_rfc3339(),
                r#"{"kind":"future_record"}"#
            ],
        )
        .unwrap();
    drop(store);
    let (kernel, store) = kernel(dir.path(), ScriptedProvider::new(vec![])).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !kernel
            .status()
            .await
            .unwrap()
            .sessions
            .iter()
            .any(|status| status.session.id == session && status.stopped.is_some())
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        kernel
            .submit(session, EventId::new(), "hello".into(), Vec::new())
            .await,
        Err(KernelError::SessionStopped(..))
    ));
    assert!(kernel.cancel_schedule(schedule).await.unwrap());
    // The cancel holds from its acceptance, before the Session runs again to record it.
    assert!(kernel.schedules(None).await.unwrap().is_empty());
    let due_at = Utc::now();
    let reminder = Event {
        id: EventId::reminder(schedule, due_at),
        session,
        source: EventSource::Scheduler,
        body: EventBody::Reminder {
            schedule,
            due_at,
            skipped: 0,
            text: "briefing".into(),
        },
        received_at: due_at,
    };
    assert_eq!(
        store.accept(&[reminder], None).await.unwrap().events,
        [Accepted::Inactive]
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn compacted_requests_are_recorded_and_the_summary_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("fs_list", r#"{"path":"."}"#)]),
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
    let mut rx = kernel.subscribe(session).await.unwrap();
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
    assert_eq!(attempts.len(), 4);
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
    let mut rx = kernel.subscribe(session).await.unwrap();
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
    let kernel = new_york(root, provider.clone(), clock).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).await.unwrap();
    let daily = json!({ "cron": "0 8 * * *", "message": "daily briefing" });
    let briefing = schedule(&kernel, &provider, &mut rx, session, daily).await;
    provider.script([reply("reminded")]);
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
    provider.script([reply("reminded")]);
    *clock.0.lock().unwrap() = utc("2026-03-10T12:01:00Z");
    let kernel = new_york(root, provider, clock).await;
    let mut rx = kernel.subscribe(session).await.unwrap();
    finish(&mut rx).await;
    (kernel, rx)
}

#[tokio::test(start_paused = true)]
async fn recurring_schedules_resume_from_their_last_delivery_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(utc("2026-03-06T13:01:00Z"))));
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, session, briefing) =
        deliver_first_briefing(dir.path(), provider.clone(), &clock).await;
    assert_eq!(briefing.next_due, utc("2026-03-07T13:00:00Z"));
    let next = kernel.schedules(None).await.unwrap().remove(0);
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
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, session, briefing) =
        deliver_first_briefing(dir.path(), provider.clone(), &clock).await;
    let (kernel, mut rx) =
        restart_after_missed_mornings(dir.path(), provider.clone(), &clock, kernel, session).await;
    let inspection = kernel.inspect(session, None).await.unwrap();
    let fixed = json!({
        "cron": "0 8 * * *",
        "timezone": "America/New_York",
        "message": "fixed zone",
    });
    let fixed = schedule(&kernel, &provider, &mut rx, session, fixed).await;
    provider.script([reply("reminded")]);
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
    let active = kernel.schedules(None).await.unwrap();
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
    let mut rx = kernel.subscribe(session.id).await.unwrap();
    let cases = [
        json!({ "cron": "invalid" }),
        json!({ "cron": "0 8 * * *", "timezone": "Unknown/Zone" }),
        json!({ "cron": "0 0 30 2 *" }),
        json!({ "cron": "* 8 * * *" }),
        json!({ "cron": "0 8 * * *", "at": "2026-10-10T08:00:00-04:00" }),
        json!({}),
        json!({ "at": "2026-10-10T08:00:00-04:00", "timezone": "America/New_York" }),
        json!({ "at": "2026-10-09T07:59:00-04:00" }),
    ];
    for mut args in cases {
        args["message"] = json!("briefing");
        provider.script([
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
    provider.script([
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
