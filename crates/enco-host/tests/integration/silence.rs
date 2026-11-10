//! A Session may skip a reply only while no interactive input awaits an answer.
#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support::*;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, oneshot};

pub(super) fn skip(note: &str) -> Result<Completion, Failure> {
    let mut completion = calls(vec![call("reply_skip", "{}")])?;
    completion
        .message
        .parts
        .insert(0, Part::Text { text: note.into() });
    Ok(completion)
}

fn skip_failure(entries: &[Entry]) {
    assert!(!entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::ToolCallStarted { capability, .. } if capability.name == "reply_skip"
    )));
    assert!(entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::ToolCallSettled { outcome: Settlement::Failed { failure }, .. }
        if failure.code == code::TOOL_REFUSED && !failure.retryable
    )));
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    assert!(entries.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RoundEnded {
            end: RoundEnd::Replied,
            ..
        }
    )));
}

#[tokio::test]
async fn an_owner_message_cannot_be_skipped_and_its_run_eventually_replies() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![skip("no"), reply("answer")]);
    let (kernel, _) = kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).await.unwrap();
    kernel
        .submit(session, EventId::new(), "question".into(), vec![])
        .await
        .unwrap();
    skip_failure(&finish(&mut rx).await);
    assert!(
        provider
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.tools.iter().any(|tool| tool.name == "reply_skip"))
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_requires_a_reply_even_when_it_returns_to_a_reminder() {
    for interactive in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let setup = if interactive {
            Vec::new()
        } else {
            scheduling().into()
        };
        let provider = ScriptedProvider::new(
            setup
                .into_iter()
                .chain([reply("first"), skip("no"), reply("again")])
                .collect(),
        );
        let (kernel, _) = kernel(dir.path(), provider).await;
        let session = kernel.open_session("main").await.unwrap().id;
        let mut rx = kernel.subscribe(session).await.unwrap();
        if interactive {
            kernel
                .submit(session, EventId::new(), "question".into(), vec![])
                .await
                .unwrap();
        } else {
            let schedule = agent_schedule(&kernel, session, &mut rx).await;
            remind(&kernel, session, schedule).await;
        }
        let entries = finish(&mut rx).await;
        let to = entries
            .iter()
            .find(|entry| matches!(entry.body, EntryBody::EventConsumed { .. }))
            .unwrap()
            .pos;
        kernel
            .backtrack(session, EventId::new(), Some(to))
            .await
            .unwrap();
        skip_failure(&finish(&mut rx).await);
        kernel.shutdown().await.unwrap();
    }
}

/// Holds the next request after `hold` open, so an input can arrive at a deterministic Round
/// boundary.
struct HeldProvider {
    scripted: Arc<ScriptedProvider>,
    entered: Notify,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

impl HeldProvider {
    fn new(scripted: Arc<ScriptedProvider>) -> Arc<Self> {
        Arc::new(Self {
            scripted,
            entered: Notify::new(),
            release: Mutex::new(None),
        })
    }

    /// Hold the next request until the returned sender fires.
    fn hold(&self) -> oneshot::Sender<()> {
        let (release, gate) = oneshot::channel();
        *self.release.lock().unwrap() = Some(gate);
        release
    }
}

#[async_trait]
impl Provider for HeldProvider {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
        record: Option<Recording>,
    ) -> Result<Answered, StoreError> {
        let release = self.release.lock().unwrap().take();
        if let Some(release) = release {
            self.entered.notify_one();
            release.await.unwrap();
        }
        self.scripted.complete(settings, request, record).await
    }
}

#[tokio::test]
async fn an_owner_input_arriving_mid_round_is_answered_in_the_same_run() {
    for first_skip in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let first = if first_skip {
            skip("checked, all fine")
        } else {
            calls(vec![call("schedule_list", "{}")])
        };
        let [create, scheduled] = scheduling();
        let scripted = ScriptedProvider::new(vec![
            create,
            scheduled,
            first,
            skip("cannot skip now"),
            reply("answer"),
        ]);
        let provider = HeldProvider::new(scripted.clone());
        let (kernel, _) = kernel(dir.path(), provider.clone()).await;
        let session = kernel.open_session("main").await.unwrap().id;
        let mut rx = kernel.subscribe(session).await.unwrap();
        let schedule = agent_schedule(&kernel, session, &mut rx).await;
        let release = provider.hold();
        remind(&kernel, session, schedule).await;
        provider.entered.notified().await;
        kernel
            .submit(session, EventId::new(), "question".into(), vec![])
            .await
            .unwrap();
        release.send(()).unwrap();
        let entries = finish(&mut rx).await;
        // The first successful skip may close a Round, but pending input keeps this Run alive.
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(
                    entry.body,
                    EntryBody::RoundEnded {
                        end: RoundEnd::ReplySkipped,
                        ..
                    }
                ))
                .count(),
            usize::from(first_skip)
        );
        let after_first_round = entries
            .iter()
            .position(|entry| matches!(entry.body, EntryBody::RoundEnded { .. }))
            .unwrap()
            + 1;
        skip_failure(&entries[after_first_round..]);
        assert!(
            // After the two requests that created the schedule and the reminder's first.
            scripted.requests.lock().unwrap()[3]
                .messages
                .iter()
                .any(|message| message.joined_text() == "question")
        );
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn skip_fails_among_other_calls_without_stopping_them_across_batches() {
    for skip_first in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let skip = call("reply_skip", "{}");
        let list = call("schedule_list", "{}");
        let shell = call("shell_exec", r#"{"command":"printf done"}"#);
        let proposed = if skip_first {
            vec![skip.clone(), list.clone(), shell.clone()]
        } else {
            vec![list.clone(), shell.clone(), skip.clone()]
        };
        let [create, scheduled] = scheduling();
        let provider =
            ScriptedProvider::new(vec![create, scheduled, calls(proposed), reply("done")]);
        let (kernel, _) = kernel(dir.path(), provider).await;
        let session = kernel.open_session("main").await.unwrap().id;
        let mut rx = kernel.subscribe(session).await.unwrap();
        let schedule = agent_schedule(&kernel, session, &mut rx).await;
        remind(&kernel, session, schedule).await;
        let entries = finish(&mut rx).await;
        assert!(!entries.iter().any(|entry| matches!(&entry.body,
            EntryBody::ToolCallStarted { call, .. } if *call == skip.id
        )));
        assert!(entries.iter().any(|entry| matches!(&entry.body,
            EntryBody::ToolCallSettled { call, outcome: Settlement::Failed { failure }, .. }
            if *call == skip.id && failure.code == code::TOOL_REFUSED && !failure.retryable
        )));
        for expected in [list.id, shell.id] {
            assert!(entries.iter().any(|entry| matches!(&entry.body,
                EntryBody::ToolCallStarted { call, .. } if *call == expected
            )));
            assert!(entries.iter().any(|entry| matches!(&entry.body,
            EntryBody::ToolCallSettled { call, outcome: Settlement::Ok, .. } if *call == expected
        )));
        }
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn an_automatic_rollback_notice_can_end_without_a_reply() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![skip("Rollback noted.")]);
    let (kernel, store) = kernel(dir.path(), provider).await;
    let session = store
        .ensure_session(&"main".parse().unwrap(), Utc::now())
        .await
        .unwrap()
        .id;
    let trial = submit(kernel.registry(), source_request("custom", FACTORY_BYTES))
        .await
        .unwrap()
        .generations
        .remove(0);
    kernel
        .registry()
        .report(
            &trial.id.into(),
            Verdict::Failed(Failure {
                code: code::PLUGIN_TRAP.into(),
                message: "fixture trapped".into(),
                retryable: false,
            }),
            Some(session),
        )
        .await
        .unwrap();
    kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session).await.unwrap();
    let entries = finish(&mut rx).await;
    assert!(entries.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::EventConsumed {
            event: Event {
                source: EventSource::Registry,
                body: EventBody::PluginRolledBack { .. },
                ..
            }
        }
    )));
    assert!(entries.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RoundEnded {
            end: RoundEnd::ReplySkipped,
            ..
        }
    )));
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_reminder_after_the_reply_may_be_skipped_in_the_same_run() {
    let dir = tempfile::tempdir().unwrap();
    let [create, scheduled] = scheduling();
    let scripted = ScriptedProvider::new(vec![
        create,
        scheduled,
        reply("first answer"),
        skip("nothing new"),
    ]);
    let provider = HeldProvider::new(scripted);
    let (kernel, _) = kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).await.unwrap();
    let schedule = agent_schedule(&kernel, session, &mut rx).await;
    let release = provider.hold();
    kernel
        .submit(session, EventId::new(), "question".into(), vec![])
        .await
        .unwrap();
    provider.entered.notified().await;
    remind(&kernel, session, schedule).await;
    release.send(()).unwrap();
    let entries = finish(&mut rx).await;
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
            .count(),
        1
    );
    let ends: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::RoundEnded { end, .. } => Some(end.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ends, [RoundEnd::Replied, RoundEnd::ReplySkipped]);
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_reminder_after_a_failed_run_may_be_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let failure = Failure {
        code: "provider.unavailable".into(),
        message: "down".into(),
        retryable: false,
    };
    // The failed combination stays exhausted in safe mode until the next Run.
    let [create, scheduled] = scheduling();
    let provider = ScriptedProvider::new(vec![
        create,
        scheduled,
        Err(failure),
        skip("all quiet after failure"),
    ]);
    let (kernel, _) = kernel(dir.path(), provider).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).await.unwrap();
    let schedule = agent_schedule(&kernel, session, &mut rx).await;
    kernel
        .submit(session, EventId::new(), "question".into(), vec![])
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Failed { .. },
            ..
        }
    ));
    remind(&kernel, session, schedule).await;
    let entries = finish(&mut rx).await;
    assert!(entries.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RoundEnded {
            end: RoundEnd::ReplySkipped,
            ..
        }
    )));
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}
