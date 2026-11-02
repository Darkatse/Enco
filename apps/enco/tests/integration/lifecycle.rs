#![expect(clippy::unwrap_used, reason = "test fixtures fail with context")]
use crate::{protocol::Command, support::*};
use enco_core::{Entry, EntryBody, EventBody, SessionRecord};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot};
use tokio_util::sync::CancellationToken;
use wiremock::{Mock, MockServer, matchers::path};

async fn command(home: &Path, args: &[&str]) -> std::process::Output {
    tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
            .args(args)
            .env("ENCO_HOME", home)
            .output(),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn cli(home: &Path, args: &[&str]) -> Value {
    let output = command(home, args).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn halt_waits_for_the_run_and_send_wait_returns_its_halted_outcome() {
    let embeddings_server = MockServer::start().await;
    embeddings(&embeddings_server).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let server_stop = stop.clone();
    let (arrived, request) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.read_exact(&mut [0; 1]).await.unwrap();
        arrived.send(()).unwrap();
        server_stop.cancelled().await;
        drop(stream);
    });
    let config = Daemon::config(&embeddings_server, "openai-compatible").replacen(
        &format!("base_url = {:?}", embeddings_server.uri()),
        &format!("base_url = \"http://{address}\""),
        1,
    );
    let mut daemon = Daemon::start_config(&config).await;
    let home = daemon.root.path().to_owned();
    let sender = tokio::spawn(async move { cli(&home, &["send", "--wait", "keep working"]).await });
    tokio::time::timeout(Duration::from_secs(30), request)
        .await
        .unwrap()
        .unwrap();
    let running = cli(daemon.root.path(), &["sessions"]).await;
    let id = running[0]["id"].as_str().unwrap();
    assert_eq!(running[0]["running"], true);
    assert!(running[0]["waiting"].is_null());
    let halted = cli(daemon.root.path(), &["halt"]).await;
    assert_eq!(halted["halted"]["sessions"][0]["id"], id);
    assert_eq!(halted["halted"]["stopped"], json!([]));
    let waited = sender.await.unwrap();
    assert_eq!(waited["end"]["kind"], "halted");
    let idle = cli(daemon.root.path(), &["sessions"]).await;
    assert_eq!(idle[0]["running"], false);
    assert!(idle[0]["waiting"].is_null());
    let entries: Vec<Entry> =
        serde_json::from_value(cli(daemon.root.path(), &["log", "--session", id]).await).unwrap();
    assert!(entries.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::EventConsumed { event } if matches!(&event.body, EventBody::Halt(..))
    )));
    stop.cancel();
    server.await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn archive_releases_the_name_but_keeps_history_addressable_by_id() {
    let service = MockServer::start().await;
    embeddings(&service).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({ "role": "assistant", "content": "done" }),
            "stop",
        ))
        .mount(&service)
        .await;
    let mut daemon = Daemon::start(&service, "openai-compatible").await;
    let home = daemon.root.path();
    let waited = cli(home, &["send", "--wait", "hello"]).await;
    let id = waited["session_id"].as_str().unwrap();
    let inspection = cli(home, &["inspect"]).await;
    assert!(!command(home, &["archive"]).await.status.success());
    assert_eq!(cli(home, &["sessions"]).await[0]["id"], id);
    assert!(
        !command(home, &["send", "--session", "main/missing", "hello"])
            .await
            .status
            .success()
    );
    assert_eq!(
        cli(home, &["archive", "main"]).await["archived"]["sessions"][0]["id"],
        id
    );
    assert!(
        !command(home, &["send", "--session", id, "hello"])
            .await
            .status
            .success()
    );
    assert_eq!(cli(home, &["sessions"]).await, json!([]));
    let archived = cli(home, &["sessions", "--archived"]).await;
    let records: Vec<SessionRecord> = serde_json::from_value(archived).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id.to_string(), id);
    assert!(records[0].archived_at.is_some());
    let entries: Vec<Entry> =
        serde_json::from_value(cli(home, &["log", "--session", id]).await).unwrap();
    assert!(!entries.is_empty());
    assert_eq!(cli(home, &["inspect", "--session", id]).await, inspection);
    let by_name = command(home, &["log", "--session", "main"]).await;
    assert!(!by_name.status.success());
    assert!(String::from_utf8_lossy(&by_name.stderr).contains(id));
    daemon
        .connect()
        .await
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let active = cli(home, &["sessions"]).await;
    assert_eq!(active[0]["name"], "main");
    assert_ne!(active[0]["id"], id);
    let by_name = cli(home, &["log", "--session", "main"]).await;
    assert_eq!(by_name, json!([]));
    assert_eq!(
        cli(home, &["log", "--session", id]).await,
        serde_json::to_value(entries).unwrap()
    );
    daemon.stop().await;
}
