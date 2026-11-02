//! Delegation exercises real Session actors, SQLite commits and the provider boundary.
#![expect(
    clippy::unwrap_used,
    reason = "integration coordination fails by panicking"
)]
use crate::support::*;
use async_trait::async_trait;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

mod lifecycle;
mod permissions;
mod recovery;

#[derive(Default)]
struct Recorder {
    inputs: Mutex<Vec<ComposeInput>>,
    /// Disclose only `session_send` to root Sessions, as a facade's composer would.
    facade: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl Composer for Recorder {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        self.inputs.lock().unwrap().push(input.clone());
        let mut plan = reply_plan(input);
        if self.facade.load(std::sync::atomic::Ordering::SeqCst) && input.session.parent.is_none() {
            plan.tools.retain(|tool| tool.name == "session_send");
        }
        Ok(Composition::Plan(plan))
    }
}
struct Harness {
    kernel: Arc<Kernel>,
    store: Arc<SqliteStore>,
    prompts: HashMap<String, mpsc::UnboundedReceiver<Prompt>>,
    composer: Arc<Recorder>,
}
impl Harness {
    async fn new(root: &std::path::Path, configure: impl FnOnce(&mut serde_json::Value)) -> Self {
        Self::with_tools(root, vec![], configure).await
    }
    async fn with_tools(
        root: &std::path::Path,
        tools: Vec<Arc<dyn Tool>>,
        configure: impl FnOnce(&mut serde_json::Value),
    ) -> Self {
        let mut document = configuration();
        let mut senders = HashMap::new();
        let mut prompts = HashMap::new();
        for model in ["fixture", "worker", "second", "third"] {
            let (send, receive) = mpsc::unbounded_channel();
            senders.insert(model.into(), send);
            prompts.insert(model.into(), receive);
            if model != "fixture" {
                document["endpoint"][model] = document["endpoint"]["reply"].clone();
                document["endpoint"][model]["model"] = model.into();
                document["profile"][model] =
                    serde_json::json!({"reply": [model], "compaction": ["compaction"]});
            }
        }
        configure(&mut document);
        let composer = Arc::new(Recorder::default());
        let (kernel, store) = kernel_configured_composer(
            root,
            Arc::new(ProviderQueues(senders)),
            document,
            tools,
            composer.clone(),
            |_| {},
        )
        .await;
        Self {
            kernel: Arc::new(kernel),
            store,
            prompts,
            composer,
        }
    }
    async fn next(&mut self, model: &str) -> Prompt {
        tokio::time::timeout(
            Duration::from_secs(15),
            self.prompts.get_mut(model).unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap()
    }
    fn latest(&self, name: &str) -> ComposeInput {
        self.composer
            .inputs
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|input| input.session.name.as_str() == name)
            .unwrap()
            .clone()
    }
    async fn child(&self, name: &str) -> SessionRecord {
        self.kernel.find_session(name).await.unwrap()
    }
}
fn send(to: &str, text: &str, profile: Option<&str>) -> ToolCall {
    let mut args = serde_json::json!({"to": to, "text": text});
    if let Some(profile) = profile {
        args["profile"] = profile.into();
    }
    call("session_send", &args.to_string())
}
async fn start(harness: &Harness) -> (SessionId, tokio::sync::broadcast::Receiver<Entry>) {
    let parent = harness.kernel.open_session("main").await.unwrap().id;
    let rx = harness.kernel.subscribe(parent).unwrap();
    harness
        .kernel
        .submit(parent, EventId::new(), "delegate work".into(), vec![])
        .await
        .unwrap();
    (parent, rx)
}
/// The words a Session has received from its children, consumed or still in its Inbox.
async fn reports(h: &Harness, session: SessionId) -> Vec<String> {
    let consumed = h.kernel.log(session, None).await.unwrap().into_iter();
    let consumed = consumed.filter_map(|entry| match entry.body {
        EntryBody::EventConsumed { event } => Some(event),
        _ => None,
    });
    consumed
        .chain(h.store.pending(session).await.unwrap())
        .filter_map(|event| match (event.source, event.body) {
            (EventSource::Child(_), EventBody::SessionMessage(outgoing)) => Some(outgoing.text),
            _ => None,
        })
        .collect()
}
/// Whether any input this Log consumed came from a child Session.
fn from_child(entries: &[Entry]) -> bool {
    entries.iter().any(|entry| {
        matches!(
            &entry.body,
            EntryBody::EventConsumed {
                event: Event {
                    source: EventSource::Child(_),
                    ..
                }
            }
        )
    })
}
fn received(entries: &[Entry], text: &str) -> bool {
    entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::EventConsumed { event: Event { source: EventSource::Child(_), body: EventBody::SessionMessage(outgoing), .. } }
        if outgoing.text == text))
}

#[tokio::test]
async fn background_reports_mid_run_messages_and_followups_share_persistent_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (parent, mut parent_rx) = start(&h).await;
    h.next("fixture").await.respond(calls(vec![
        send("research", "first task", Some("worker")),
        send("research", "second task", None),
    ]));
    let parent_ready = h.next("fixture").await;
    let worker = h.next("worker").await;
    let child = h.child("main/research").await;
    let mut child_rx = h.kernel.subscribe(child.id).unwrap();
    assert_eq!(child.parent.as_ref().unwrap().id, parent);
    assert_eq!(child.working_dir, h.child("main").await.working_dir);
    parent_ready.respond(reply("working in the background"));
    let parent_entries = finish(&mut parent_rx).await;
    assert_eq!(
        parent_entries
            .iter()
            .filter(|entry| matches!(
                &entry.body,
                EntryBody::ToolCallSettled {
                    outcome: Settlement::Ok,
                    delegated: Some(_),
                    ..
                }
            ))
            .count(),
        2
    );
    let finding = dir.path().join("finding.txt");
    std::fs::write(&finding, b"data").unwrap();
    worker.respond(calls(vec![call(
        "message_send",
        &serde_json::json!({"text": "early finding", "file": finding}).to_string(),
    )]));
    let parent_report = h.next("fixture").await;
    let child_next = h.next("worker").await;
    assert!(parent_report.contains("early finding"));
    // The parent's composer gets the child's file at a local path it can read.
    assert!(h.latest("main").transcript.items.iter().any(|item| {
        matches!(&item.event, Some(Event { body: EventBody::SessionMessage(Outgoing { file: Some(file), .. }), .. }) if file.name == "finding.txt")
            && item
                .file_path
                .as_ref()
                .is_some_and(|path| std::fs::read(path).unwrap() == b"data")
    }));
    assert!(h.latest("main").children[0].state.awaiting);
    parent_report.respond(crate::silence::skip("not worth interrupting"));
    assert!(received(&finish(&mut parent_rx).await, "early finding"));
    child_next.respond(reply("finished research"));
    finish(&mut child_rx).await;
    let report = h.next("fixture").await;
    assert!(report.contains("finished research"));
    assert!(!h.latest("main").children[0].state.awaiting);
    report.respond(reply("verified result"));
    assert!(received(&finish(&mut parent_rx).await, "finished research"));
    h.kernel
        .submit(
            parent,
            EventId::new(),
            "continue the research".into(),
            vec![],
        )
        .await
        .unwrap();
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "followup task", None)]));
    let worker = h.next("worker").await;
    assert!(worker.contains("first task"));
    assert!(worker.contains("second task"));
    assert!(worker.contains("finished research"));
    assert!(worker.contains("followup task"));
    h.next("fixture").await.respond(reply("continuing"));
    finish(&mut parent_rx).await;
    worker.respond(reply("followup answer"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("checked"));
    finish(&mut parent_rx).await;
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn owner_takeover_changes_reply_destination_until_the_parent_sends_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (parent, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let worker = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    let child = h.child("main/research").await;
    h.kernel
        .submit(child.id, EventId::new(), "owner takeover".into(), vec![])
        .await
        .unwrap();
    worker.respond(calls(vec![call("schedule_list", "{}")]));
    let worker = h.next("worker").await;
    assert!(worker.contains("owner takeover"));
    crate::silence::reminder(&h.kernel, parent).await;
    let parent_prompt = h.next("fixture").await;
    let state = &h.latest("main").children[0];
    assert!(state.state.replies_to_owner);
    assert!(!state.state.awaiting);
    parent_prompt.respond(crate::silence::skip("owner took over"));
    finish(&mut rx).await;
    let mut child_rx = h.kernel.subscribe(child.id).unwrap();
    worker.respond(reply("answer for owner"));
    finish(&mut child_rx).await;
    // The answer went to the owner in the commit that ended its Round; the parent never had it.
    assert!(
        !reports(&h, parent)
            .await
            .contains(&"answer for owner".to_owned())
    );
    h.kernel
        .submit(parent, EventId::new(), "resume task".into(), vec![])
        .await
        .unwrap();
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "back to parent", None)]));
    let worker = h.next("worker").await;
    assert!(!h.latest("main").children[0].state.replies_to_owner);
    h.next("fixture").await.respond(reply("resumed"));
    finish(&mut rx).await;
    worker.respond(reply("parent result"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("accepted"));
    assert!(received(&finish(&mut rx).await, "parent result"));
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_or_cancelled_child_runs_report_their_outcome_to_the_parent() {
    for cancel in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut h = Harness::new(dir.path(), |_| {}).await;
        let (_, mut rx) = start(&h).await;
        h.next("fixture")
            .await
            .respond(calls(vec![send("research", "task", Some("worker"))]));
        let worker = h.next("worker").await;
        h.next("fixture").await.respond(reply("delegated"));
        finish(&mut rx).await;
        let child = h.child("main/research").await;
        if cancel {
            assert!(h.kernel.cancel(child.id).unwrap());
        } else {
            worker.respond(Err(Failure {
                code: "provider.failed".into(),
                message: "worker failed".into(),
                retryable: false,
            }));
        }
        h.next("fixture")
            .await
            .respond(crate::silence::skip("failure noted"));
        let entries = finish(&mut rx).await;
        assert!(entries.iter().any(|entry| matches!(&entry.body,
            EntryBody::EventConsumed { event: Event { source: EventSource::Child(_), body: EventBody::ChildRunEnded { end }, .. } }
            if if cancel { matches!(end, RunEnd::Cancelled) } else { matches!(end, RunEnd::Failed { .. }) })));
        assert!(!h.latest("main").children[0].state.awaiting);
        h.kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_new_childs_relative_working_directory_resolves_against_its_parents_unchecked() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (_, mut rx) = start(&h).await;
    let args = serde_json::json!({
        "to": "research",
        "text": "task",
        "profile": "worker",
        "working_dir": "not/yet",
    });
    h.next("fixture")
        .await
        .respond(calls(vec![call("session_send", &args.to_string())]));
    let held = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    let main = h.child("main").await;
    let child = h.child("main/research").await;
    assert_eq!(
        child.working_dir.path(),
        main.working_dir.path().join("not/yet")
    );
    // Creation does not check the directory; the calls that use it do.
    assert!(!child.working_dir.path().exists());
    drop(held);
    h.kernel.shutdown().await.unwrap();
}
