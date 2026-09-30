#![expect(clippy::unwrap_used, reason = "test coordination fails by panicking")]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::channel::{Action, Adapter, Channel, Fault, Identity, Input, Update};
use enco_kernel::{ConnectionWrite, Store};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::*;
use tokio::sync::{Mutex, mpsc, oneshot};

struct Send {
    target: String,
    text: String,
    result: oneshot::Sender<Result<(), Fault>>,
}

struct ControlledChannel {
    updates: Mutex<mpsc::Receiver<Vec<Update>>>,
    polls: mpsc::UnboundedSender<Value>,
    sends: mpsc::UnboundedSender<Send>,
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
    fn split(&self, text: &str) -> Vec<String> {
        vec![text.into()]
    }
    async fn send(&self, target: &str, text: &str) -> Result<(), Fault> {
        let (result, receive) = oneshot::channel();
        self.sends
            .send(Send {
                target: target.into(),
                text: text.into(),
                result,
            })
            .unwrap();
        receive.await.unwrap()
    }
}

async fn receive<T>(receiver: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}

fn update(offset: u32, action: Action) -> Update {
    Update {
        protocol: json!(offset),
        input: Some(Input {
            conversation: "chat".into(),
            sender: Some("owner".into()),
            conversation_type: "private".into(),
            direct: true,
            action: Some(action),
        }),
    }
}

#[tokio::test]
async fn connection_accepts_while_sending_and_preserves_unknown_outcomes_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("first"), reply("second"), reply("cli only")]);
    let (kernel, store) = kernel(dir.path(), provider.clone()).await;
    let kernel = Arc::new(kernel);
    let (updates, rx) = mpsc::channel(4);
    let (polls, mut polling) = mpsc::unbounded_channel();
    let (sends, mut sending) = mpsc::unbounded_channel();
    let adapter = Arc::new(ControlledChannel {
        updates: Mutex::new(rx),
        polls,
        sends,
    });
    let channel = Channel::start(
        kernel.clone(),
        adapter.clone(),
        "owner".into(),
        Arc::new(enco_host::SystemClock),
    );
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
        .send(vec![update(2, Action::Message("again".into()))])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(2));
    first
        .result
        .send(Err(Fault {
            failure: Failure {
                code: "connection_lost".into(),
                message: "response was lost".into(),
                retryable: true,
            },
            unknown: true,
            fatal: false,
            retry_after: None,
        }))
        .unwrap();
    let second = receive(&mut sending).await;
    assert_eq!(second.text, "second");
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
    assert!(matches!(&failures[0].outcome, Settlement::Unknown { failure } if !failure.retryable));
    let session = kernel.open_session("main").await.unwrap();
    assert_eq!(failures[0].delivery.session, session.id);
    let log = kernel.log(session.id, None).await.unwrap();
    assert!(log.iter().any(|entry| entry.pos == failures[0].delivery.pos
        && matches!(
            entry.body,
            EntryBody::RoundEnded {
                end: RoundEnd::Replied,
                ..
            }
        )));
    assert_eq!(provider.requests.lock().unwrap().len(), 2);

    // Reopening retains the failure while processed replies are not sent again.
    let channel = Channel::start(
        kernel.clone(),
        adapter.clone(),
        "owner".into(),
        Arc::new(enco_host::SystemClock),
    );
    assert_eq!(receive(&mut polling).await, json!(3));
    let mut entries = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "local".into())
        .await
        .unwrap();
    finish(&mut entries).await;
    updates
        .send(vec![update(4, Action::Sessions)])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(4));
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "* main");
    receipt.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    assert_eq!(channel.status().await.unwrap().recent_failures, failures);

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
    let channel = Channel::start(
        kernel.clone(),
        adapter,
        "owner".into(),
        Arc::new(enco_host::SystemClock),
    );
    assert_eq!(receive(&mut polling).await, json!(4));
    let status = channel.status().await.unwrap();
    assert_eq!(status.recent_failures.len(), 2);
    assert!(
        matches!(&status.recent_failures[0].outcome, Settlement::Unknown { failure } if failure.code == code::INTERRUPTED)
    );
    // An operational failure, whether observed before or during shutdown, is status, not a shutdown error.
    updates
        .send(vec![update(5, Action::Sessions)])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(5));
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
    assert_eq!(status.recent_failures.len(), 2); // Command receipts never become Log deliveries.
    kernel.shutdown().await.unwrap();
}
