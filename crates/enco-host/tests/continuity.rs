#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
mod support;

use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use support::*;

struct TestClock(Mutex<DateTime<FixedOffset>>);

impl Clock for TestClock {
    fn now(&self) -> DateTime<FixedOffset> {
        *self.0.lock().unwrap()
    }
}

impl TestClock {
    fn advance(&self, seconds: i64) {
        *self.0.lock().unwrap() += chrono::Duration::seconds(seconds);
    }
}

#[tokio::test]
async fn requests_keep_a_stable_prefix_and_date_inputs_in_the_current_offset() {
    let dir = tempfile::tempdir().unwrap();
    let first = DateTime::parse_from_rfc3339("2026-09-29T13:00:00-04:00").unwrap();
    let clock = Arc::new(TestClock(Mutex::new(first)));
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
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps| {
        deps.clock = clock.clone();
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    // (clock at submission, whether history contains a failed Run, expected local arrival times)
    let cases: [(&str, bool, &[&str]); 5] = [
        ("2026-09-29T13:00:00-04:00", false, &["2026-09-29 13:00"]),
        ("2026-09-29T13:59:00-04:00", false, &["2026-09-29 13:00"]),
        (
            "2026-09-29T14:01:00-04:00",
            true,
            &["2026-09-29 13:00", "2026-09-29 14:01"],
        ),
        (
            "2026-09-29T16:01:00-04:00",
            true,
            &["2026-09-29 13:00", "2026-09-29 14:01", "2026-09-29 16:01"],
        ),
        (
            "2026-09-29T22:02:00+02:00",
            true,
            &["2026-09-29 19:00", "2026-09-29 20:01", "2026-09-29 22:01"],
        ),
    ];
    let mut previous_offset = None;
    for (at, has_failure, expected_times) in cases {
        let now = DateTime::parse_from_rfc3339(at).unwrap();
        *clock.0.lock().unwrap() = now;
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
            .filter(|text| text.contains("2026-09-29 "))
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
    let clock = Arc::new(TestClock(Mutex::new(Utc::now().fixed_offset())));
    let provider = ScriptedProvider::new(vec![reply("first reminder"), reply("overdue reminder")]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps| {
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
                clock.now().to_utc() - chrono::Duration::seconds(1),
                "past".into()
            )
            .await,
        Err(ScheduleError::InPast(_))
    ));
    let first = kernel
        .schedules()
        .create(
            session.id,
            clock.now().to_utc() + chrono::Duration::seconds(10),
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
            .filter(|entry| is_reminder(entry, first.id))
            .count(),
        1
    );
    let cancelled = kernel
        .schedules()
        .create(
            session.id,
            clock.now().to_utc() + chrono::Duration::seconds(10),
            "cancelled".into(),
        )
        .await
        .unwrap();
    assert!(kernel.schedules().cancel(cancelled.id).await.unwrap());
    assert!(!kernel.schedules().cancel(cancelled.id).await.unwrap());
    let overdue = kernel
        .schedules()
        .create(
            session.id,
            clock.now().to_utc() + chrono::Duration::seconds(20),
            "overdue".into(),
        )
        .await
        .unwrap();
    kernel.shutdown().await.unwrap();
    drop(kernel);
    clock.advance(30);
    let (kernel, _) = kernel_with(dir.path(), provider, |deps| deps.clock = clock.clone()).await;
    let mut rx = kernel.subscribe(session.id).unwrap();
    let log = kernel.log(session.id, None).await.unwrap();
    if !log.iter().any(|entry| is_reminder(entry, overdue.id)) {
        finish(&mut rx).await;
    }
    kernel.shutdown().await.unwrap();
    let log = kernel.log(session.id, None).await.unwrap();
    for id in [first.id, overdue.id] {
        assert_eq!(log.iter().filter(|entry| is_reminder(entry, id)).count(), 1);
    }
    assert!(!log.iter().any(|entry| is_reminder(entry, cancelled.id)));
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
        DateTime::parse_from_rfc3339("2026-09-29T09:00:00-04:00").unwrap(),
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
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), configure).await;
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
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), configure).await;
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
