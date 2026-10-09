#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
mod support;

use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use support::*;

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

#[tokio::test]
async fn requests_keep_a_stable_prefix_and_date_inputs_across_dst() {
    let dir = tempfile::tempdir().unwrap();
    let first = DateTime::parse_from_rfc3339("2026-11-01T00:00:00-04:00").unwrap();
    let clock = Arc::new(TestClock(Mutex::new(first.to_utc())));
    let provider = ScriptedProvider::new(vec![
        reply("first"),
        Err(Failure {
            code: "provider.bad_request".into(),
            message: "request rejected".into(),
            retryable: false,
        }),
        reply("recovered"),
        reply("later"),
        reply("new offset"),
    ]);
    let (kernel, _) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        |deps| {
            deps.clock = clock.clone();
        },
    )
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    // (clock at submission, whether history contains a failed Run, expected local arrival times)
    let cases: [(&str, bool, &[&str]); 5] = [
        ("2026-11-01T00:00:00-04:00", false, &["2026-11-01 00:00"]),
        ("2026-11-01T00:59:00-04:00", false, &["2026-11-01 00:00"]),
        (
            "2026-11-01T01:01:00-04:00",
            true,
            &["2026-11-01 00:00", "2026-11-01 01:01"],
        ),
        // The repeated hour starts a new marker although the local hour is unchanged.
        (
            "2026-11-01T01:20:00-05:00",
            true,
            &["2026-11-01 00:00", "2026-11-01 01:01", "2026-11-01 01:20"],
        ),
        (
            "2026-11-01T03:00:00-05:00",
            true,
            &[
                "2026-11-01 00:00",
                "2026-11-01 01:01",
                "2026-11-01 01:20",
                "2026-11-01 03:00",
            ],
        ),
    ];
    let mut previous_offset = None;
    for (at, has_failure, expected_times) in cases {
        let now = DateTime::parse_from_rfc3339(at).unwrap();
        *clock.0.lock().unwrap() = now.to_utc();
        let inputs = ["time", "also"].map(|text| Event {
            id: EventId::new(),
            session: session.id,
            source: EventSource::Cli,
            body: EventBody::UserMessage { text: text.into() },
            received_at: now.to_utc(),
        });
        kernel.accept(&inputs, None).await.unwrap();
        finish(&mut rx).await;
        let requests = provider.requests.lock().unwrap();
        let request = requests.last().unwrap();
        assert_eq!(request.messages.last().unwrap().joined_text(), "also");
        let actual: Vec<_> = request
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .map(Message::joined_text)
            .filter(|text| text.contains("2026-11-01 "))
            .collect();
        assert_eq!(actual.len(), expected_times.len());
        for (note, expected) in actual.iter().zip(expected_times) {
            assert!(note.contains(expected));
        }
        // The whole recorded request stays a prefix until the offset changes the head.
        if previous_offset == Some(*now.offset()) {
            let previous = &requests[requests.len() - 2];
            assert!(request.messages.starts_with(&previous.messages));
        }
        previous_offset = Some(*now.offset());
        let failures: Vec<_> = request
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.joined_text().contains("request rejected"))
            .collect();
        assert_eq!(failures.len(), usize::from(has_failure));
        if let Some((index, message)) = failures.first() {
            assert!(message.joined_text().contains("provider.bad_request"));
            assert_eq!(request.messages[index + 1].joined_text(), "time");
        }
    }
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
async fn small_window_compaction_preserves_context_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(
        (0..30)
            .map(|_| reply("Concise summary or reply."))
            .collect(),
    );
    provider.steps.lock().unwrap().push_front(Err(Failure {
        code: "provider.bad_request".into(),
        message: "request rejected".into(),
        retryable: false,
    }));
    let (embedding_registry, _) = registry(
        &dir.path().join("embedding-registry"),
        Loaded {
            lifecycle: Arc::new(Probe(Ok(()))),
            summary: "embedding".into(),
            completion: None,
            embedding: Some(provider.clone()),
            decision: None,
        },
    )
    .await;
    let memories = Memories::open(
        MemoryPaths {
            db: dir.path().join("memory.db"),
            index: dir.path().join("memory-index"),
        },
        embedding_endpoint("fixture", 64),
        embedding_registry,
        Arc::new(SystemClock),
    )
    .await
    .unwrap();
    memories.save("Owner is River".into(), true).await.unwrap();
    let clock = Arc::new(TestClock(Mutex::new(
        DateTime::parse_from_rfc3339("2026-09-29T09:00:00-04:00")
            .unwrap()
            .to_utc(),
    )));
    let timestamp = "2026-09-29 09:00";
    let compaction_window = 2500;
    let configure = |deps: &mut KernelDeps| {
        deps.clock = clock.clone();
        deps.context
            .push(Arc::new(MemoryContextSource::new(memories.clone(), None)));
        deps.tools.extend(memory_tools(memories.clone()));
        let profile = deps.profiles.get_mut("default").unwrap();
        profile.reply.budget = Budget {
            context_tokens: 6000,
            max_output_tokens: 256,
        };
        profile.compaction.budget = Budget {
            context_tokens: compaction_window,
            max_output_tokens: 512,
        };
    };
    let (kernel, _) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        configure,
    )
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "History before failure".into())
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Failed { .. },
            ..
        }
    ));
    for n in 0..6 {
        kernel
            .submit(
                session.id,
                EventId::new(),
                format!("History {n}: {}", "some detailed history ".repeat(150)),
            )
            .await
            .unwrap();
        assert!(matches!(
            finish(&mut rx).await.last().unwrap().body,
            EntryBody::RunEnded {
                end: RunEnd::Completed,
                ..
            }
        ));
    }
    let log = kernel.log(session.id, None).await.unwrap();
    let requests = provider.requests.lock().unwrap().clone();
    let attempts: Vec<_> = log
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted { attempt, .. } => Some(attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts.len(), requests.len());
    let mut summarized_outcome = false;
    for (attempt, request) in attempts.into_iter().zip(requests) {
        let inspection = kernel.inspect(session.id, Some(attempt)).await.unwrap();
        assert_eq!(inspection.request, request);
        if inspection.purpose == AttemptPurpose::Compaction {
            let prompt: u32 = request
                .messages
                .iter()
                .map(|message| estimate_tokens(&message.joined_text()))
                .sum();
            assert!(prompt + request.max_output_tokens.unwrap() <= compaction_window);
            let text = request.messages[1].joined_text();
            assert!(text.contains(timestamp));
            assert!(!text.contains("Owner is River"));
            summarized_outcome |=
                text.contains("provider.bad_request") && text.contains("request rejected");
        }
    }
    assert!(summarized_outcome);
    let summary = log
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            EntryBody::Compacted { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .expect("history did not compact");
    kernel.shutdown().await.unwrap();
    drop(kernel);
    let (kernel, _) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        configure,
    )
    .await;
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "What do you remember?".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    let request = provider.requests.lock().unwrap().last().unwrap().clone();
    assert!(
        request
            .messages
            .first()
            .unwrap()
            .joined_text()
            .contains("Owner is River")
    );
    assert!(request.messages[0].joined_text().contains(&summary));
    assert!(request.messages[1].joined_text().contains(timestamp));
    kernel.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn recurring_schedules_resume_from_last_delivery_and_follow_the_owner_zone() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(
        "2026-03-06T13:01:00Z".parse().unwrap(),
    )));
    let provider = ScriptedProvider::new((0..3).map(|_| reply("reminded")).collect());
    let (kernel, _) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        |deps| deps.clock = clock.clone(),
    )
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    let recurring = kernel
        .schedules()
        .create(
            session.id,
            ScheduleRule::Cron {
                expr: "0 8 * * *".into(),
                timezone: None,
            },
            "daily briefing".into(),
        )
        .await
        .unwrap();
    let id = recurring.schedule.id;
    assert_eq!(
        recurring.next_due,
        "2026-03-07T13:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    *clock.0.lock().unwrap() = recurring.next_due;
    tokio::time::advance(Duration::from_secs(1)).await;
    finish(&mut rx).await;
    let next = kernel.schedules().list(None).await.unwrap().remove(0);
    assert_eq!(
        next.next_due,
        "2026-03-08T12:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    kernel.shutdown().await.unwrap();
    drop(kernel);

    // Three missed local mornings, including the DST change, become one input.
    *clock.0.lock().unwrap() = "2026-03-10T12:01:00Z".parse().unwrap();
    let (kernel, _) = kernel_with(
        dir.path(),
        provider.clone(),
        Tz::America__New_York,
        |deps| deps.clock = clock.clone(),
    )
    .await;
    let mut rx = kernel.subscribe(session.id).unwrap();
    finish(&mut rx).await;
    let log = kernel.log(session.id, None).await.unwrap();
    let reminders: Vec<_> = log.iter().filter(|entry| is_reminder(entry, id)).collect();
    assert_eq!(reminders.len(), 2);
    assert!(
        matches!(&reminders[1].body, EntryBody::EventConsumed { event: Event { body: EventBody::Reminder { skipped: 2, due_at, .. }, .. } }
        if *due_at == "2026-03-10T12:00:00Z".parse::<DateTime<Utc>>().unwrap())
    );
    let inspection = kernel.inspect(session.id, None).await.unwrap();
    let fixed = kernel
        .schedules()
        .create(
            session.id,
            ScheduleRule::Cron {
                expr: "0 8 * * *".into(),
                timezone: Some(Tz::America__New_York),
            },
            "fixed zone".into(),
        )
        .await
        .unwrap();
    kernel.shutdown().await.unwrap();
    drop(kernel);

    // A westward zone change uses the same catch-up rule; an explicit zone stays fixed.
    *clock.0.lock().unwrap() = "2026-03-10T16:00:00Z".parse().unwrap();
    let (kernel, _) = kernel_with(dir.path(), provider, Tz::America__Los_Angeles, |deps| {
        deps.clock = clock.clone()
    })
    .await;
    let mut rx = kernel.subscribe(session.id).unwrap();
    finish(&mut rx).await;
    assert_eq!(
        kernel
            .inspect(session.id, Some(inspection.attempt))
            .await
            .unwrap()
            .request,
        inspection.request
    );
    let current = kernel.inspect(session.id, None).await.unwrap().request;
    assert!(
        current.messages[0]
            .joined_text()
            .contains("America/Los_Angeles")
    );
    // Arrival and planned times are both narrated again in the new zone.
    let rendered: Vec<_> = current.messages.iter().map(Message::joined_text).collect();
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("2026-03-07 05:00"))
    );
    assert!(
        !rendered
            .iter()
            .any(|text| text.contains("2026-03-07 08:00"))
    );
    let active = kernel.schedules().list(None).await.unwrap();
    let following = active.iter().find(|s| s.schedule.id == id).unwrap();
    assert_eq!(
        following.next_due,
        "2026-03-11T15:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
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
            .submit(session.id, EventId::new(), "set a reminder".into())
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
        .submit(session.id, EventId::new(), "use eight o'clock".into())
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
