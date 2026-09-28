#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::*;
use tokio::sync::Notify;

struct PendingProvider {
    entered: Notify,
    dropped: Notify,
}

#[async_trait]
impl Provider for PendingProvider {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "pending-provider".into(),
            version: "test".into(),
        }
    }

    async fn complete(&self, _: ProviderRequest) -> Result<Completion, Failure> {
        let _dropped = Dropped(&self.dropped);
        self.entered.notify_one();
        std::future::pending().await
    }

    async fn embed(&self, _: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        std::future::pending().await
    }
}

struct GatedTool {
    effect: Effect,
    entered: Notify,
    dropped: Notify,
    finished: AtomicBool,
}

#[async_trait]
impl Tool for GatedTool {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "gated-tool".into(),
            version: "test".into(),
        }
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "gated".into(),
            description: "Test operation which finishes a local write after cancellation.".into(),
            input_schema: serde_json::json!({ "type": "object" }),
            effect: self.effect,
        }
    }

    async fn call(
        &self,
        ctx: CallContext,
        _: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        let _dropped = Dropped(&self.dropped);
        self.entered.notify_one();
        ctx.cancel.cancelled().await;
        self.finished.store(true, Ordering::SeqCst);
        Outcome::Ok {
            value: serde_json::json!("local write completed"),
        }
    }
}

struct GatedContext {
    entered: Notify,
    finished: AtomicBool,
}

#[async_trait]
impl ContextSource for GatedContext {
    async fn contribute(&self, query: &ContextQuery) -> Result<Contribution, ContextError> {
        self.entered.notify_one();
        query.cancel.cancelled().await;
        self.finished.store(true, Ordering::SeqCst);
        Err(ContextError("cancelled".into()))
    }
}

async fn recovered(kernel: &Kernel, session: SessionId) -> Vec<Entry> {
    let mut rx = kernel.subscribe(session).unwrap();
    let log = kernel.log(session, None).await.unwrap();
    if !log.iter().any(|e| {
        matches!(
            e.body,
            EntryBody::RunEnded {
                end: RunEnd::Interrupted,
                ..
            }
        )
    }) {
        finish(&mut rx).await;
    }
    kernel.log(session, None).await.unwrap()
}

#[tokio::test]
async fn aborted_provider_request_is_settled_without_automatic_resumption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_owned();
    let provider = Arc::new(PendingProvider {
        entered: Notify::new(),
        dropped: Notify::new(),
    });
    let remote = provider.clone();
    let worker = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let (old, _, session) = worker
        .spawn(async move {
            let (kernel, store) = kernel(&path, remote).await;
            let session = kernel.open_session("main").await.unwrap();
            (kernel, store, session)
        })
        .await
        .unwrap();
    old.submit(session.id, EventId::new(), "hello".into())
        .await
        .unwrap();
    observed(&provider.entered).await;
    worker.shutdown_background();
    observed(&provider.dropped).await;
    drop(old);
    let next = ScriptedProvider::new(vec![reply("recovered")]);
    let (kernel, _) = kernel(dir.path(), next.clone()).await;
    let log = recovered(&kernel, session.id).await;
    assert!(log.iter().any(|entry| match &entry.body {
        EntryBody::AttemptSettled {
            result: AttemptResult::Failed { failure },
            ..
        } => failure.code == code::INTERRUPTED,
        _ => false,
    }));
    assert!(
        !log.iter()
            .any(|e| { matches!(e.body, EntryBody::ToolCallStarted { .. }) })
    );
    assert!(next.requests.lock().unwrap().is_empty());
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "continue".into())
        .await
        .unwrap();
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
async fn recovery_distinguishes_started_effects_from_calls_never_dispatched() {
    for effect in [Effect::SideEffect, Effect::ReadOnly] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_owned();
        let tool = Arc::new(GatedTool {
            effect,
            entered: Notify::new(),
            dropped: Notify::new(),
            finished: AtomicBool::new(false),
        });
        let remote = tool.clone();
        let first = call("gated", "{}");
        let second = call("fs_write", r#"{"path":"must-not-exist","content":"wrong"}"#);
        let provider = ScriptedProvider::new(vec![calls(vec![first.clone(), second.clone()])]);
        let worker = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (old, _, session) = worker
            .spawn(async move {
                let (kernel, store) =
                    kernel_with(&path, provider, |deps, _| deps.tools.push(remote)).await;
                let session = kernel.open_session("main").await.unwrap();
                (kernel, store, session)
            })
            .await
            .unwrap();
        old.submit(session.id, EventId::new(), "two calls".into())
            .await
            .unwrap();
        observed(&tool.entered).await;
        worker.shutdown_background();
        observed(&tool.dropped).await;
        drop(old);
        let (kernel, _) = kernel(dir.path(), ScriptedProvider::new(vec![])).await;
        let log = recovered(&kernel, session.id).await;
        let first_outcome = log
            .iter()
            .find_map(|e| match &e.body {
                EntryBody::ToolCallSettled { call, outcome, .. } if *call == first.id => {
                    Some(outcome)
                }
                _ => None,
            })
            .unwrap();
        assert!(matches!(
            (effect, first_outcome),
            (Effect::SideEffect, Outcome::Unknown { .. })
                | (Effect::ReadOnly, Outcome::Failed { .. })
        ));
        assert!(log.iter().any(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                call,
                outcome: Outcome::Failed { failure },
                ..
            } => *call == second.id && failure.code == code::NOT_DISPATCHED,
            _ => false,
        }));
        assert!(!dir.path().join("workspace/must-not-exist").exists());
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_reaches_context_and_shutdown_waits_for_tool_settlement() {
    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(GatedContext {
        entered: Notify::new(),
        finished: AtomicBool::new(false),
    });
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps, _| {
        deps.context = vec![source.clone()]
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "hello".into())
        .await
        .unwrap();
    observed(&source.entered).await;
    assert!(kernel.cancel(session.id).unwrap());
    let log = finish(&mut rx).await;
    assert!(source.finished.load(Ordering::SeqCst));
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
    drop(kernel);
    let dir = tempfile::tempdir().unwrap();
    let tool = Arc::new(GatedTool {
        effect: Effect::SideEffect,
        entered: Notify::new(),
        dropped: Notify::new(),
        finished: AtomicBool::new(false),
    });
    let (kernel, _) = kernel_with(
        dir.path(),
        ScriptedProvider::new(vec![calls(vec![call("gated", "{}")])]),
        |deps, _| deps.tools.push(tool.clone()),
    )
    .await;
    let session = kernel.open_session("main").await.unwrap();
    kernel
        .submit(session.id, EventId::new(), "work".into())
        .await
        .unwrap();
    observed(&tool.entered).await;
    kernel.shutdown().await.unwrap();
    assert!(tool.finished.load(Ordering::SeqCst));
    let log = kernel.log(session.id, None).await.unwrap();
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    assert!(log.iter().any(|e| {
        matches!(
            e.body,
            EntryBody::ToolCallSettled {
                outcome: Outcome::Ok { .. },
                ..
            }
        )
    }));
}

#[tokio::test]
async fn safe_mode_bypasses_broken_context_and_normal_mode_can_resume() {
    let dir = tempfile::tempdir().unwrap();
    let at = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call(
            "schedule_create",
            &serde_json::json!({ "at": at, "message": "must not be created" }).to_string(),
        )]),
        reply("safe"),
        reply("normal"),
    ]);
    let (kernel, store) = kernel(dir.path(), provider.clone()).await;
    std::fs::write(dir.path().join("workspace/AGENTS.md"), [0xff]).unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "hello".into())
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Failed { .. },
            ..
        }
    ));
    kernel.set_safe_mode(true).await.unwrap();
    kernel
        .submit(session.id, EventId::new(), "repair".into())
        .await
        .unwrap();
    let log = finish(&mut rx).await;
    assert!(log.iter().any(|e| {
        matches!(
            e.body,
            EntryBody::RoundStarted {
                safe_mode: true,
                ..
            }
        )
    }));
    assert!(kernel.schedules().list(None).await.unwrap().is_empty());
    assert!(
        !log.iter()
            .any(|e| { matches!(e.body, EntryBody::ToolCallStarted { .. }) })
    );
    assert!(log.iter().any(|entry| match &entry.body {
        EntryBody::ToolCallSettled {
            outcome: Outcome::Failed { failure },
            ..
        } => failure.code == code::TOOL_UNAVAILABLE,
        _ => false,
    }));
    let hash = log
        .iter()
        .find_map(|e| match e.body {
            EntryBody::AttemptStarted { plan, .. } => Some(plan),
            _ => None,
        })
        .unwrap();
    let plan: ContextPlan = serde_json::from_slice(&store.get_blob(&hash).await.unwrap()).unwrap();
    assert_eq!(
        plan.tools
            .iter()
            .map(|(_, t)| t.name.as_str())
            .collect::<Vec<_>>(),
        enco_host::LIFELINE
    );
    std::fs::write(
        dir.path().join("workspace/AGENTS.md"),
        "Call the owner River.",
    )
    .unwrap();
    kernel.set_safe_mode(false).await.unwrap();
    kernel
        .submit(session.id, EventId::new(), "hello again".into())
        .await
        .unwrap();
    finish(&mut rx).await;
    assert!(
        provider.requests.lock().unwrap().last().unwrap().messages[0]
            .joined_text()
            .contains("Call the owner River.")
    );
    kernel.shutdown().await.unwrap();
}

struct Dropped<'a>(&'a Notify);

impl Drop for Dropped<'_> {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

async fn observed(notify: &Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(15), notify.notified())
        .await
        .expect("test operation did not reach its observation point");
}
