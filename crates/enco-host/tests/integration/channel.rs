#![expect(clippy::unwrap_used, reason = "test coordination fails by panicking")]
mod backtrack;
mod outgoing;
use crate::support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::{
    SqliteStore,
    channel::{
        Action, Adapter, Attachment, AttachmentContent, Channel, Fault, Identity, Input,
        MessageContent, Update,
    },
};
use enco_kernel::{ConnectionWrite, Kernel, Store};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use support::*;
use tokio::sync::{Mutex, mpsc, oneshot};

struct Send {
    target: String,
    text: String,
    file: Option<FileData>,
    /// The ID the service gives the message if the send succeeds.
    id: String,
    result: oneshot::Sender<Result<(), Fault>>,
}

struct Fetch {
    locator: String,
    result: oneshot::Sender<Result<AttachmentContent, Fault>>,
}

struct ControlledChannel {
    updates: Mutex<mpsc::Receiver<Vec<Update>>>,
    polls: mpsc::UnboundedSender<Value>,
    sends: mpsc::UnboundedSender<Send>,
    fetches: mpsc::UnboundedSender<Fetch>,
    sent: AtomicU32,
}

#[async_trait]
impl Adapter for ControlledChannel {
    fn identity(&self) -> Identity {
        Identity {
            channel: "controlled".into(),
            account: "bot".into(),
        }
    }
    fn initial_state(&self) -> Value {
        json!(0)
    }
    async fn poll(&self, protocol: Value) -> Result<Vec<Update>, Fault> {
        self.polls.send(protocol).unwrap();
        Ok(self.updates.lock().await.recv().await.unwrap())
    }
    async fn fetch(&self, locator: &str) -> Result<AttachmentContent, Fault> {
        let (result, receive) = oneshot::channel();
        self.fetches
            .send(Fetch {
                locator: locator.into(),
                result,
            })
            .unwrap();
        receive.await.unwrap()
    }
    fn split(&self, message: Outgoing<FileData>) -> Vec<Outgoing<FileData>> {
        vec![message]
    }
    async fn send(&self, target: &str, message: &Outgoing<FileData>) -> Result<String, Fault> {
        let (result, receive) = oneshot::channel();
        let id = format!("sent-{}", self.sent.fetch_add(1, Ordering::SeqCst));
        self.sends
            .send(Send {
                target: target.into(),
                text: message.text.clone(),
                file: message.file.clone(),
                id: id.clone(),
                result,
            })
            .unwrap();
        receive.await.unwrap().map(|()| id)
    }
}

/// A node with a controlled adapter: the test feeds its updates and observes its polls and sends.
struct Harness {
    kernel: Arc<Kernel>,
    store: Arc<SqliteStore>,
    adapter: Arc<ControlledChannel>,
    updates: mpsc::Sender<Vec<Update>>,
    polling: mpsc::UnboundedReceiver<Value>,
    sending: mpsc::UnboundedReceiver<Send>,
    fetching: mpsc::UnboundedReceiver<Fetch>,
}

async fn harness(root: &Path, provider: Arc<ScriptedProvider>) -> Harness {
    let (kernel, store) = kernel(root, provider).await;
    let (updates, rx) = mpsc::channel(4);
    let (polls, polling) = mpsc::unbounded_channel();
    let (sends, sending) = mpsc::unbounded_channel();
    let (fetches, fetching) = mpsc::unbounded_channel();
    let adapter = Arc::new(ControlledChannel {
        updates: Mutex::new(rx),
        polls,
        sends,
        fetches,
        sent: AtomicU32::new(0),
    });
    Harness {
        kernel: Arc::new(kernel),
        store,
        adapter,
        updates,
        polling,
        sending,
        fetching,
    }
}

fn start(kernel: &Arc<Kernel>, adapter: &Arc<ControlledChannel>) -> Channel {
    Channel::start(
        kernel.clone(),
        adapter.clone(),
        "owner".into(),
        Arc::new(enco_host::SystemClock),
    )
}

async fn receive<T>(receiver: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}

/// The owner's message in `chat`; the protocol's ID for it is its offset.
fn update(offset: u32, action: Action) -> Update {
    Update {
        protocol: json!(offset),
        input: Some(Input {
            conversation: "chat".into(),
            message: offset.to_string(),
            sender: Some("owner".into()),
            conversation_type: "private".into(),
            direct: true,
            action: Some(action),
        }),
    }
}

/// A send whose response was lost, so the service may or may not have delivered it.
fn lost_response() -> Fault {
    Fault {
        failure: Failure {
            code: "connection_lost".into(),
            message: "response was lost".into(),
            retryable: true,
        },
        unknown: true,
        fatal: false,
        retry_after: None,
    }
}

#[tokio::test]
async fn connection_accepts_while_sending_and_preserves_unknown_outcomes_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let markdown = "```rust\nlet x = 1;\n```\n\n# Second";
    let provider = ScriptedProvider::new(vec![
        reply("first"),
        reply(markdown),
        calls(vec![call("message_send", r#"{"text":"cli progress"}"#)]),
        reply("cli only"),
    ]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider.clone()).await;
    let channel = start(&kernel, &adapter);
    assert_eq!(receive(&mut polling).await, json!(0));
    let mut foreign = update(0, Action::Message("unauthorized".into()));
    foreign.input.as_mut().unwrap().sender = Some("another user".into());
    let mut group = update(0, Action::Message("group input".into()));
    group.input.as_mut().unwrap().direct = false;
    group.input.as_mut().unwrap().conversation_type = "group".into();
    updates
        .send(vec![
            foreign,
            group,
            update(1, Action::Message("hello".into())),
        ])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(1));
    let first = receive(&mut sending).await;
    assert_eq!((&*first.target, &*first.text), ("chat", "first"));
    let before_send = kernel.connection("controlled:bot").await.unwrap().unwrap();
    assert!(!before_send["sending"].is_null());

    // The previous send remains in flight while input is accepted and its cursor committed.
    updates
        .send(vec![
            update(2, Action::Message("again".into())),
            update(2, Action::SelectSession("work".into())),
        ])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(2));
    first.result.send(Err(lost_response())).unwrap();
    let switched = receive(&mut sending).await;
    assert_eq!(switched.text, "Switched to work.");
    switched.result.send(Ok(())).unwrap();
    let second = receive(&mut sending).await;
    assert_eq!(second.text, format!("[main]\n\n{markdown}"));
    second.result.send(Ok(())).unwrap();
    // A command receipt follows the completed logical delivery, without asking the model.
    updates
        .send(vec![update(3, Action::Sessions)])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(3));
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let failures = channel.status().await.unwrap().recent_failures;
    assert_eq!(failures.len(), 1);
    assert!(
        matches!(&failures[0].settlement.outcome, Settlement::Unknown { failure } if !failure.retryable)
    );
    let session = kernel.open_session("main").await.unwrap();
    assert_eq!(failures[0].settlement.delivery.session, session.id);
    let log = kernel.log(session.id, None).await.unwrap();
    assert!(
        log.iter()
            .any(|entry| entry.pos == failures[0].settlement.delivery.pos
                && matches!(
                    entry.body,
                    EntryBody::RoundEnded {
                        end: RoundEnd::Replied,
                        ..
                    }
                ))
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 2);

    // Reopening retains the failure while processed replies are not sent again.
    let channel = start(&kernel, &adapter);
    assert_eq!(receive(&mut polling).await, json!(3));
    let mut entries = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "local".into(), Vec::new())
        .await
        .unwrap();
    finish(&mut entries).await;
    updates
        .send(vec![update(4, Action::Sessions)])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(4));
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "- main\n- **work** (current)");
    receipt.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    assert_eq!(
        serde_json::to_value(channel.status().await.unwrap().recent_failures).unwrap(),
        serde_json::to_value(failures).unwrap()
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_delivery_left_unsettled_by_a_crash_is_recorded_as_unknown_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("first"), reply("cli only")]);
    let Harness {
        kernel,
        store,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider).await;
    // An earlier delivery whose outcome is unknown; shutdown settles it.
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("hello".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending)
        .await
        .result
        .send(Err(lost_response()))
        .unwrap();
    channel.shutdown().await.unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut entries = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "local".into(), Vec::new())
        .await
        .unwrap();
    finish(&mut entries).await;

    // Seed the exact durable start left by a process that died before a final receipt.
    let pos = kernel
        .log(session.id, None)
        .await
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|e| matches!(e.body, EntryBody::RunEnded { .. }).then_some(e.pos))
        .unwrap();
    let mut state = kernel.connection("controlled:bot").await.unwrap().unwrap();
    state["sending"] = json!(Delivery {
        session: session.id,
        pos,
        target: "chat".into()
    });
    store
        .accept(
            &[],
            Some(&ConnectionWrite {
                key: "controlled:bot".into(),
                state,
                settlement: None,
            }),
        )
        .await
        .unwrap();
    let channel = start(&kernel, &adapter);
    assert_eq!(receive(&mut polling).await, json!(1));
    let status = channel.status().await.unwrap();
    assert_eq!(status.recent_failures.len(), 2);
    assert!(
        matches!(&status.recent_failures[0].settlement.outcome, Settlement::Unknown { failure } if failure.code == code::INTERRUPTED)
    );
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_fatal_send_failure_stops_the_connection_as_status() {
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    // An operational failure, whether observed before or during shutdown, is status, not a shutdown error.
    updates
        .send(vec![update(1, Action::Sessions)])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(1));
    receive(&mut sending)
        .await
        .result
        .send(Err(Fault {
            failure: Failure {
                code: "channel.http.401".into(),
                message: "credentials rejected".into(),
                retryable: false,
            },
            unknown: false,
            fatal: true,
            retry_after: None,
        }))
        .unwrap();
    channel.shutdown().await.unwrap();
    let status = channel.status().await.unwrap();
    assert!(!status.running);
    assert!(status.stopped.unwrap().contains("channel.http.401"));
    assert!(status.recent_failures.is_empty()); // Command receipts never become Log deliveries.
    kernel.shutdown().await.unwrap();
}

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3, 2, 0,
    239, 163, 175, 75, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn photo() -> Action {
    Action::Message(MessageContent {
        text: "caption".into(),
        images: vec![Attachment {
            what: "photo".into(),
            locator: "file-id".into(),
        }],
        unviewable: Vec::new(),
    })
}

#[tokio::test]
async fn attachments_are_authorized_before_fetch_and_retry_after_the_committed_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        reply("seed"),
        reply("first"),
        reply("image"),
        reply("unavailable"),
    ]);
    let Harness {
        kernel,
        store,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut fetching,
    } = harness(dir.path(), provider).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(0, Action::Message("seed".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let mut foreign = update(1, photo());
    foreign.input.as_mut().unwrap().sender = Some("stranger".into());
    updates
        .send(vec![
            foreign,
            update(2, Action::Message("first".into())),
            update(3, photo()),
        ])
        .await
        .unwrap();
    let fetch = receive(&mut fetching).await;
    assert_eq!(fetch.locator, "file-id");
    // A pending download leaves the connection free to deliver an earlier Round.
    let delivery = receive(&mut sending).await;
    assert_eq!(delivery.text, "seed");
    delivery.result.send(Ok(())).unwrap();
    let mut error = lost_response();
    error.retry_after = Some(Duration::ZERO);
    fetch.result.send(Err(error)).unwrap();
    assert_eq!(receive(&mut polling).await, json!(2));
    assert!(fetching.try_recv().is_err());
    receive(&mut sending).await.result.send(Ok(())).unwrap();

    updates.send(vec![update(3, photo())]).await.unwrap();
    let bytes = PNG.to_vec();
    receive(&mut fetching)
        .await
        .result
        .send(Ok(AttachmentContent::Image(bytes.clone())))
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(3));
    receive(&mut sending).await.result.send(Ok(())).unwrap();

    updates.send(vec![update(4, photo())]).await.unwrap();
    receive(&mut fetching)
        .await
        .result
        .send(Ok(AttachmentContent::Unavailable(
            "getFile returned HTTP 403".into(),
        )))
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(4));
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let events: Vec<_> = kernel
        .log(session.id, None)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.body {
            EntryBody::EventConsumed { event } => Some(event.body),
            _ => None,
        })
        .collect();
    assert_eq!(events.len(), 4);
    let EventBody::UserMessage {
        text,
        images,
        unviewable,
    } = &events[2]
    else {
        panic!("expected owner message")
    };
    assert_eq!(text, "caption");
    assert!(unviewable.is_empty());
    assert_eq!(images.len(), 1);
    assert_eq!(store.get_blob(&images[0].content).await.unwrap(), bytes);
    let EventBody::UserMessage {
        images, unviewable, ..
    } = &events[3]
    else {
        panic!("expected owner message")
    };
    assert!(images.is_empty());
    assert!(
        matches!(&unviewable[0].reason, UnviewableReason::Unavailable(reason) if reason.contains("403"))
    );
    kernel.shutdown().await.unwrap();
}
