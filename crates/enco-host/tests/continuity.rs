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
    // (clock at submission, whether the previous Run failed, markers expected in the history)
    let cases: [(&str, bool, &[&str]); 5] = [
        (
            "2026-09-29T13:00:00-04:00",
            false,
            &["[2026-09-29 13:00, Tuesday]"],
        ),
        (
            "2026-09-29T13:59:00-04:00",
            false,
            &["[2026-09-29 13:00, Tuesday]"],
        ),
        (
            "2026-09-29T14:01:00-04:00",
            true,
            &["[2026-09-29 13:00, Tuesday]", "[2026-09-29 14:01, Tuesday]"],
        ),
        (
            "2026-09-29T16:01:00-04:00",
            false,
            &[
                "[2026-09-29 13:00, Tuesday]",
                "[2026-09-29 14:01, Tuesday]",
                "[2026-09-29 16:01, Tuesday]",
            ],
        ),
        (
            "2026-09-29T22:02:00+02:00",
            false,
            &[
                "[2026-09-29 19:00, Tuesday]",
                "[2026-09-29 20:01, Tuesday]",
                "[2026-09-29 22:01, Tuesday]",
            ],
        ),
    ];
    let mut previous_offset = None;
    for (at, after_failure, markers) in cases {
        let now = DateTime::parse_from_rfc3339(at).unwrap();
        *clock.0.lock().unwrap() = now;
        kernel
            .submit(session.id, EventId::new(), "time".into())
            .await
            .unwrap();
        finish(&mut rx).await;
        let requests = provider.requests.lock().unwrap();
        let request = requests.last().unwrap();
        let (context, prefix) = request.messages.split_last().unwrap();
        assert!(context.joined_text().contains(at));
        let actual: Vec<_> = prefix
            .iter()
            .filter(|m| m.role == Role::User)
            .map(Message::joined_text)
            .filter(|text| text.starts_with('['))
            .collect();
        assert_eq!(actual, markers);
        // While the offset holds, only the final context message differs from the last request.
        if previous_offset == Some(*now.offset()) {
            let previous = &requests[requests.len() - 2];
            assert!(prefix.starts_with(&previous.messages[..previous.messages.len() - 1]));
            assert_eq!(request.tools, previous.tools);
        }
        previous_offset = Some(*now.offset());
        assert_eq!(
            context.joined_text().contains("request rejected"),
            after_failure
        );
        assert!(
            !prefix
                .iter()
                .any(|m| m.joined_text().contains("request rejected"))
        );
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
async fn compaction_survives_restart_and_preserves_memory_without_hidden_log_references() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(
        (0..30)
            .map(|_| reply("Concise summary or reply."))
            .collect(),
    );
    let (embedding_registry, _) = registry(
        &dir.path().join("embedding-registry"),
        Loaded {
            lifecycle: Arc::new(Probe(Ok(()))),
            summary: "embedding".into(),
            completion: None,
            embedding: Some(provider.clone()),
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
    let marker = "[2026-09-29 09:00, Tuesday]";
    let configure = |deps: &mut KernelDeps| {
        deps.clock = clock.clone();
        deps.context
            .push(Arc::new(MemoryContextSource::new(memories.clone())));
        deps.tools.extend(memory_tools(memories.clone()));
        let profile = deps.profiles.get_mut("default").unwrap();
        profile.reply.budget = Budget {
            context_tokens: 6000,
            max_output_tokens: 256,
        };
        profile.reply.settings.model = "reply".into();
        profile.compaction.settings.model = "compaction".into();
        profile.compaction.budget.max_output_tokens = 512;
    };
    let (kernel, store) = kernel_with(dir.path(), provider.clone(), configure).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
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
    let settings = provider.settings.lock().unwrap().clone();
    for ((attempt, request), settings) in attempts.into_iter().zip(requests).zip(settings) {
        let inspection = kernel.inspect(session.id, Some(attempt)).await.unwrap();
        let (model, output) = match inspection.purpose {
            AttemptPurpose::Reply => ("reply", 256),
            AttemptPurpose::Compaction => ("compaction", 512),
        };
        assert_eq!(settings.model, model);
        assert_eq!(inspection.settings, settings);
        assert_eq!(request.max_output_tokens, Some(output));
        assert_eq!(inspection.request, request);
        match inspection.purpose {
            AttemptPurpose::Reply => assert_eq!(request.messages[1].joined_text(), marker),
            AttemptPurpose::Compaction => {
                let text = request.messages[1].joined_text();
                assert!(text.contains(&format!("{marker}\nOwner: History")));
                assert!(!text.contains("Owner is River"));
            }
        }
    }
    let (upto, summary) = log
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            EntryBody::Compacted { upto, summary, .. } => Some((*upto, summary.clone())),
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
    let hash = entries
        .iter()
        .rev()
        .find_map(|e| match e.body {
            EntryBody::AttemptStarted {
                plan,
                purpose: AttemptPurpose::Reply,
                ..
            } => Some(plan),
            _ => None,
        })
        .unwrap();
    let plan: ContextPlan = serde_json::from_slice(&store.get_blob(&hash).await.unwrap()).unwrap();
    assert!(plan.items.iter().all(|item| match item {
        PlanItem::Log { pos } => *pos > upto,
        _ => true,
    }));
    let request = provider.requests.lock().unwrap().last().unwrap().clone();
    assert!(
        request
            .messages
            .last()
            .unwrap()
            .joined_text()
            .contains("Owner is River")
    );
    assert!(request.messages[0].joined_text().contains(&summary));
    assert_eq!(request.messages[1].joined_text(), marker);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_smaller_compaction_window_compacts_in_steps_and_the_conversation_continues() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(
        (0..40)
            .map(|_| reply("Concise summary or reply."))
            .collect(),
    );
    let compaction_window = 2500;
    let (kernel, _) = kernel_with(dir.path(), provider, |deps| {
        let profile = deps.profiles.get_mut("default").unwrap();
        profile.reply.budget = Budget {
            context_tokens: 6000,
            max_output_tokens: 256,
        };
        profile.compaction.budget = Budget {
            context_tokens: compaction_window,
            max_output_tokens: 512,
        };
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    for n in 0..8 {
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
    let compactions: Vec<_> = log
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted {
                attempt,
                purpose: AttemptPurpose::Compaction,
                ..
            } => Some(attempt),
            _ => None,
        })
        .collect();
    assert!(!compactions.is_empty());
    for attempt in compactions {
        let request = kernel
            .inspect(session.id, Some(attempt))
            .await
            .unwrap()
            .request;
        let prompt: u32 = request
            .messages
            .iter()
            .map(|message| estimate_tokens(&message.joined_text()))
            .sum();
        assert!(prompt + request.max_output_tokens.unwrap() <= compaction_window);
    }
    kernel.shutdown().await.unwrap();
}

#[test]
fn budget_omissions_are_explicit_and_do_not_make_oversized_candidates_mandatory() {
    let input = ComposeInput {
        now: Utc::now().fixed_offset(),
        session: SessionRecord {
            id: SessionId::new(),
            name: "budget".into(),
            created_at: Utc::now(),
            binding: Binding {
                node: NodeId::new(),
                epoch: Epoch(1),
            },
            profile: "default".into(),
        },
        transcript: Transcript::default(),
        previous_run_end: None,
        context: Contribution {
            candidates: vec![
                Candidate {
                    id: "instructions:AGENTS.md".into(),
                    kind: CandidateKind::Instruction,
                    text: "large instruction ".repeat(150),
                },
                Candidate {
                    id: "memory:large".into(),
                    kind: CandidateKind::Memory,
                    text: "oversized memory ".repeat(150),
                },
                Candidate {
                    id: "memory:small".into(),
                    kind: CandidateKind::Memory,
                    text: "Owner is River".into(),
                },
            ],
            omitted: vec![],
        },
        tools: vec![],
        safe_mode: false,
        profile: Profile {
            reply: Endpoint {
                budget: Budget {
                    context_tokens: 1500,
                    max_output_tokens: 100,
                },
                ..endpoint()
            },
            ..profile()
        },
        compactions_left: 2,
    };
    let Composition::Plan(plan) = FactoryComposer::new("/workspace".into(), "/AGENTS.md".into())
        .compose(&input)
        .unwrap()
    else {
        panic!("no history requires compaction");
    };
    assert!(
        plan.omitted
            .iter()
            .any(|o| o.source == "instructions:AGENTS.md")
    );
    assert!(plan.omitted.iter().any(|o| o.source == "memory:large"));
    let PlanItem::Message { message } = plan.items.last().unwrap() else {
        panic!("missing current context");
    };
    assert!(message.joined_text().contains("Owner is River"));
    assert!(!message.joined_text().contains("oversized memory"));
}
