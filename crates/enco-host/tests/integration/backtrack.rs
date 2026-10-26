//! A backtrack moves the conversation back without rewriting the Log or stopping other work.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use support::*;
use tokio::sync::{Notify, broadcast};

/// Records the previous plan each composition receives.
#[derive(Default)]
struct Recording(Mutex<Vec<Option<ContextPlan>>>);

#[async_trait]
impl Composer for Recording {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        self.0.lock().unwrap().push(input.previous_plan.clone());
        Ok(Composition::Plan(reply_plan(input)))
    }
}

/// Memories that hold the next Round open until it is released or cancelled, once armed.
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl ContextSource for Gate {
    type Supply = OfferedMemories;

    async fn contribute(&self, query: &ContextQuery) -> Result<OfferedMemories, ContextError> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            tokio::select! {
                biased;
                () = query.cancel.cancelled() => {}
                () = self.release.notified() => {}
            }
        }
        Ok(OfferedMemories::default())
    }
}

fn texts(request: &ProviderRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .map(Message::joined_text)
        .filter(|text| !text.is_empty())
        .collect()
}

/// Positions of the Round ends that close a Run, in Log order.
fn run_replies(log: &[Entry]) -> Vec<LogPos> {
    log.windows(2)
        .filter(|pair| {
            matches!(pair[0].body, EntryBody::RoundEnded { .. })
                && matches!(pair[1].body, EntryBody::RunEnded { .. })
        })
        .map(|pair| pair[0].pos)
        .collect()
}

fn input(log: &[Entry], text: &str) -> LogPos {
    log.iter()
        .find(|entry| {
            matches!(&entry.body, EntryBody::EventConsumed { event }
            if matches!(&event.body, EventBody::UserMessage { text: t, .. } if t == text))
        })
        .unwrap()
        .pos
}

async fn consumed_backtrack(rx: &mut broadcast::Receiver<Entry>) -> LogPos {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let entry = rx.recv().await.unwrap();
            if matches!(&entry.body, EntryBody::EventConsumed { event }
                if matches!(event.body, EventBody::Backtrack { .. }))
            {
                return entry.pos;
            }
        }
    })
    .await
    .expect("backtrack was not consumed")
}

async fn send(kernel: &Kernel, session: SessionId, text: &str) {
    kernel
        .submit(session, EventId::new(), text.into(), vec![])
        .await
        .unwrap();
}

#[tokio::test]
async fn a_backtrack_to_a_reply_continues_from_it_and_can_return_to_the_left_part() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(["first", "second", "third", "fourth"].map(reply).into());
    let composer = Arc::new(Recording::default());
    let (kernel, store) = kernel_configured_composer(
        dir.path(),
        provider.clone(),
        configuration(),
        vec![],
        composer.clone(),
        |_| {},
    )
    .await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    for text in ["one", "two"] {
        send(&kernel, session, text).await;
        finish(&mut rx).await;
    }
    let log = kernel.log(session, None).await.unwrap();
    let [first, second] = run_replies(&log)[..] else {
        panic!("two replies")
    };
    let first_plan = log
        .iter()
        .find_map(|entry| match entry.body {
            EntryBody::AttemptStarted { plan, .. } => Some(plan),
            _ => None,
        })
        .unwrap();
    kernel
        .backtrack(session, EventId::new(), first)
        .await
        .unwrap();
    let backtrack = consumed_backtrack(&mut rx).await;
    send(&kernel, session, "three").await;
    finish(&mut rx).await;
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one", "first", "three"]
    );
    let first_plan = store.plan(&first_plan).await.unwrap();
    assert_eq!(
        composer.0.lock().unwrap().last().unwrap().as_ref(),
        Some(&first_plan)
    );
    // What the conversation left stays recorded, so it can be returned to.
    kernel
        .backtrack(session, EventId::new(), second)
        .await
        .unwrap();
    consumed_backtrack(&mut rx).await;
    send(&kernel, session, "four").await;
    finish(&mut rx).await;
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one", "first", "two", "second", "four"]
    );
    let missing = LogPos {
        epoch: first.epoch,
        seq: Seq(u64::MAX),
    };
    for to in [missing, backtrack] {
        assert!(matches!(
            kernel.backtrack(session, EventId::new(), to).await,
            Err(KernelError::Backtrack { .. })
        ));
    }
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_to_an_input_replies_to_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("first"), reply("again")]);
    let (kernel, _) = support::kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    send(&kernel, session, "one").await;
    finish(&mut rx).await;
    let log = kernel.log(session, None).await.unwrap();
    kernel
        .backtrack(session, EventId::new(), input(&log, "one"))
        .await
        .unwrap();
    finish(&mut rx).await;
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one"]
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_stops_the_running_run_and_supersedes_the_inputs_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate::default());
    let provider = ScriptedProvider::new(vec![reply("first"), reply("fourth")]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), Tz::UTC, |deps| {
        deps.memories = gate.clone()
    })
    .await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    send(&kernel, session, "one").await;
    finish(&mut rx).await;
    let first = run_replies(&kernel.log(session, None).await.unwrap())[0];
    gate.armed.store(true, Ordering::SeqCst);
    send(&kernel, session, "two").await;
    tokio::time::timeout(std::time::Duration::from_secs(15), gate.entered.notified())
        .await
        .unwrap();
    send(&kernel, session, "three").await;
    kernel
        .backtrack(session, EventId::new(), first)
        .await
        .unwrap();
    let ended = finish(&mut rx).await;
    assert!(matches!(
        ended.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    consumed_backtrack(&mut rx).await;
    send(&kernel, session, "four").await;
    finish(&mut rx).await;
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(texts(&requests[1]), ["one", "first", "four"]);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_resent_backtrack_leaves_a_later_run_running() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate::default());
    let provider = ScriptedProvider::new(vec![reply("first"), reply("second")]);
    let (kernel, _) = kernel_with(dir.path(), provider, Tz::UTC, |deps| {
        deps.memories = gate.clone()
    })
    .await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    send(&kernel, session, "one").await;
    finish(&mut rx).await;
    let first = run_replies(&kernel.log(session, None).await.unwrap())[0];
    let id = EventId::new();
    kernel.backtrack(session, id, first).await.unwrap();
    consumed_backtrack(&mut rx).await;
    gate.armed.store(true, Ordering::SeqCst);
    send(&kernel, session, "two").await;
    tokio::time::timeout(std::time::Duration::from_secs(15), gate.entered.notified())
        .await
        .unwrap();
    assert_eq!(
        kernel.backtrack(session, id, first).await.unwrap(),
        Accepted::Duplicate
    );
    gate.release.notify_one();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_before_a_compaction_restores_the_full_history() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("fs_list", r#"{"path":"."}"#)]),
        reply("scripted summary"),
        reply("finished"),
    ]);
    let (kernel, _) = kernel_configured_composer(
        dir.path(),
        provider,
        configuration(),
        vec![],
        Arc::new(HistoryComposer),
        |_| {},
    )
    .await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    send(&kernel, session, "initial request").await;
    let entries = finish(&mut rx).await;
    let before = entries
        .iter()
        .find(|entry| matches!(entry.body, EntryBody::RoundEnded { .. }))
        .unwrap()
        .pos;
    kernel.shutdown().await.unwrap();
    drop(kernel);
    let provider = ScriptedProvider::new(vec![reply("continued")]);
    let (kernel, _) = support::kernel(dir.path(), provider.clone()).await;
    let mut rx = kernel.subscribe(session).unwrap();
    kernel
        .backtrack(session, EventId::new(), before)
        .await
        .unwrap();
    consumed_backtrack(&mut rx).await;
    send(&kernel, session, "continue").await;
    finish(&mut rx).await;
    let request = provider.requests.lock().unwrap()[0].clone();
    assert_eq!(texts(&request), ["initial request", "continue"]);
    assert!(request.messages.iter().any(|message| {
        message
            .parts
            .iter()
            .any(|part| matches!(part, Part::ToolResult(_)))
    }));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_accepted_before_a_restart_takes_effect_once() {
    let dir = tempfile::tempdir().unwrap();
    let (kernel, store) =
        support::kernel(dir.path(), ScriptedProvider::new(vec![reply("first")])).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    send(&kernel, session, "one").await;
    finish(&mut rx).await;
    let to = input(&kernel.log(session, None).await.unwrap(), "one");
    kernel.shutdown().await.unwrap();
    drop(kernel);
    store
        .accept(
            &[Event {
                id: EventId::new(),
                session,
                source: EventSource::Cli,
                body: EventBody::Backtrack { to },
                received_at: Utc::now(),
            }],
            None,
        )
        .await
        .unwrap();
    drop(store);
    let provider = ScriptedProvider::new(vec![reply("again")]);
    let (kernel, _) = support::kernel(dir.path(), provider.clone()).await;
    let mut rx = kernel.subscribe(session).unwrap();
    let runs = |log: &[Entry]| {
        log.iter()
            .filter(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
            .count()
    };
    let log = kernel.log(session, None).await.unwrap();
    // The reply may already have finished before the subscription.
    if runs(&log) < 2 || !matches!(log.last().unwrap().body, EntryBody::RunEnded { .. }) {
        finish(&mut rx).await;
    }
    kernel.shutdown().await.unwrap();
    assert_eq!(runs(&kernel.log(session, None).await.unwrap()), 2);
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one"]
    );
}
