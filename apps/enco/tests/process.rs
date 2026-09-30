#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::*;
use protocol::Command;
use serde_json::json;
use std::{os::unix::fs::MetadataExt, path::Path, process::Stdio, time::Duration};
use support::*;
use wiremock::{Mock, MockServer, matchers::path};

async fn reject_second_serve(root: &Path) {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
            .arg("serve")
            .env("ENCO_HOME", root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("enco serve is already running"));
}

#[tokio::test]
async fn node_lock_is_held_while_serving_and_until_shutdown_is_quiescent() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            tool_message("read", "fs_read", json!({"path": "held"})),
            "tool_calls",
        ))
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    reject_second_serve(daemon.root.path()).await;
    let mut client = daemon.connect().await;
    client.request(Command::Status {}).await.unwrap();

    // fs_read does not observe cancellation (05 §2), so a FIFO read holds shutdown open
    // until this test closes its writer, without sleeps.
    let fifo = daemon.root.path().join("workspace/held");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            session: "main".into(),
            text: "read held".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let writer = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::fs::OpenOptions::new().write(true).open(&fifo),
    )
    .await
    .unwrap()
    .unwrap();
    let socket = daemon.root.path().join(".data/enco.sock");
    let inode = std::fs::metadata(&socket).unwrap().ino();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &daemon.child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while client.reader.next_line().await.unwrap().is_some() {}
    })
    .await
    .unwrap();
    reject_second_serve(daemon.root.path()).await;
    assert!(daemon.child.try_wait().unwrap().is_none());
    assert_eq!(std::fs::metadata(&socket).unwrap().ino(), inode);

    drop(writer);
    assert!(
        tokio::time::timeout(Duration::from_secs(10), daemon.child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(!socket.exists());
    let lock = std::fs::File::options()
        .write(true)
        .open(daemon.root.path().join(".data/enco.lock"))
        .unwrap();
    lock.try_lock().unwrap();
}

async fn process_model(server: &MockServer) {
    embeddings(server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.iter().any(|message| message["role"] == "tool") {
                return response(
                    json!({ "role": "assistant", "content": "recovered" }),
                    "stop",
                );
            }
            let command = "echo $$ > shell.pid; echo run >> marker.txt; \
                    printf ready > ../ready; exec sleep 60";
            let message = tool_message("shell-call", "shell_exec", json!({ "command": command }));
            response(message, "tool_calls")
        })
        .mount(server)
        .await;
}

async fn started(daemon: &Daemon, client: &mut client::Client) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(daemon.root.path().join("ready"))
            .status()
            .unwrap()
            .success()
    );
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            session: "main".into(),
            text: "run the test command".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(30),
        tokio::fs::read_to_string(daemon.root.path().join("ready")),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn killed_daemon_settles_unknown_without_reexecuting_a_side_effect() {
    let server = MockServer::start().await;
    process_model(&server).await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut client = daemon.connect().await;
    started(&daemon, &mut client).await;
    daemon.stop().await;
    let pid = std::fs::read_to_string(daemon.root.path().join("workspace/shell.pid")).unwrap();
    let _ = std::process::Command::new("kill")
        .args(["-TERM", pid.trim()])
        .status();
    daemon.restart().await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let log: Vec<Entry> = serde_json::from_value(
        client
            .request(Command::Log {
                session: "main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    if !log
        .iter()
        .any(|e| matches!(e.body, EntryBody::RunEnded { .. }))
    {
        finish(&mut client).await;
    }
    let log: Vec<Entry> = serde_json::from_value(
        client
            .request(Command::Log {
                session: "main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(log.iter().any(|entry| match &entry.body {
        EntryBody::ToolCallSettled {
            outcome: Settlement::Unknown { failure },
            ..
        } => failure.code == code::INTERRUPTED,
        _ => false,
    }));
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Interrupted,
            ..
        }
    ));
    assert_eq!(
        std::fs::read_to_string(daemon.root.path().join("workspace/marker.txt")).unwrap(),
        "run\n"
    );
    client
        .send(Command::Send {
            session: "main".into(),
            text: "continue".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut client).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    daemon.stop().await;
}

#[tokio::test]
async fn cancelling_shell_reaps_the_owned_process_before_settlement() {
    let server = MockServer::start().await;
    process_model(&server).await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut client = daemon.connect().await;
    started(&daemon, &mut client).await;
    let data = client
        .request(Command::Cancel {
            session: "main".into(),
        })
        .await
        .unwrap();
    assert_eq!(data["cancelled"], true);
    let entries = finish(&mut client).await;
    assert!(entries.iter().any(|entry| match &entry.body {
        EntryBody::ToolCallSettled {
            outcome: Settlement::Unknown { failure },
            ..
        } => failure.code == code::CANCELLED,
        _ => false,
    }));
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    let pid = std::fs::read_to_string(daemon.root.path().join("workspace/shell.pid")).unwrap();
    assert!(
        !std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    daemon.stop().await;
}
