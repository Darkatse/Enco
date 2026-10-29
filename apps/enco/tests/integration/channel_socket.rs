#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "fixture failures retain their context"
)]
use crate::support::channel;
use enco_core::{GenerationId, MappingRecord, PluginSettings};
use enco_host::{SqliteStore, StorePaths};
use enco_kernel::{ChannelAdapter, ChannelReading, ChannelSettings, Runtime, Store};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{Message, handshake::server::Request, protocol::CloseFrame},
};
use tokio_util::sync::CancellationToken;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

const SECRET: &str = "42:test-secret";

#[tokio::test]
async fn socket_frames_timers_and_resume_continue_while_receipt_acknowledgment_waits() {
    if channel::child(
        "channel_socket::socket_frames_timers_and_resume_continue_while_receipt_acknowledgment_waits",
    ) {
        return;
    }
    let http = http_server().await;
    let socket = socket_server().await;
    let cdn = MockServer::start().await;
    Mock::given(path("/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"image".as_slice()))
        .mount(&cdn)
        .await;
    let (root, store, adapter) = adapter(&http.url, &socket.url, &cdn.uri()).await;
    assert_eq!(
        adapter.identity().facts["echo"],
        "[redacted:ENCO_CHANNEL_FIXTURE_TOKEN]"
    );
    let (send, mut readings) = mpsc::unbounded_channel();
    let stop = CancellationToken::new();
    let receiving = adapter.clone();
    let cancelled = stop.clone();
    let task = tokio::spawn(async move { receiving.receive(None, send, cancelled).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        http.started.await.unwrap();
        socket.resumed.await.unwrap();
    })
    .await
    .unwrap();
    // Nothing has consumed the preparation queue, yet the socket has sent a heartbeat and resumed.
    let mut first_acknowledgment = take_messages(&mut readings, &*store).await;
    assert!(matches!(
        first_acknowledgment.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert!(
        matches!(adapter.fetch("file").await.unwrap(), enco_kernel::AttachmentContent::Image(bytes) if bytes == b"image")
    );
    assert!(
        !cdn.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization")
    );
    // A mapper that never finishes a fetch fails its generation's contract.
    assert!(matches!(
        adapter.fetch("endless").await,
        Err(enco_kernel::ChannelCallError::Protocol(fault)) if fault.failure.code == enco_core::code::PLUGIN_CONTRACT
    ));
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(10), socket.task)
        .await
        .unwrap()
        .unwrap();
    assert!(!task.is_finished());
    http.release.send(()).unwrap();
    first_acknowledgment.await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    http.task.await.unwrap();
    assert_records(root.path(), &*store).await;
}

struct HttpServer {
    url: String,
    started: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

async fn http_server() -> HttpServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (started, observed) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let task = tokio::spawn(async move {
        for (route, status, body) in [
            ("/identify", 200, json!({"echo":SECRET}).to_string()),
            ("/ack", 204, String::new()),
        ] {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                bytes.push(connection.read_u8().await.unwrap());
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.lines().next().unwrap().contains(route));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains(&format!("authorization: bot {SECRET}"))
            );
            if route == "/ack" {
                started.send(()).unwrap();
                released.await.unwrap();
                connection.write_all(format!("HTTP/1.1 {status} OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                break;
            }
            connection.write_all(format!("HTTP/1.1 {status} OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    });
    HttpServer {
        url,
        started: observed,
        release,
        task,
    }
}

struct SocketServer {
    url: String,
    resumed: oneshot::Receiver<()>,
    task: JoinHandle<()>,
}

#[expect(
    clippy::result_large_err,
    reason = "Tungstenite fixes the handshake callback error type"
)]
async fn socket_server() -> SocketServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (resumed, observed) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(stream, |request: &Request, response| {
            assert_eq!(request.uri().query(), Some("resume=null"));
            Ok(response)
        })
        .await
        .unwrap();
        socket
            .send(Message::text(json!({"seq":1,"echo":SECRET}).to_string()))
            .await
            .unwrap();
        let identify = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let identify: Value = serde_json::from_str(&identify).unwrap();
        assert_eq!(identify, json!({"op":"identify","token":SECRET}));
        socket
            .send(Message::text(json!({"seq":2,"echo":SECRET}).to_string()))
            .await
            .unwrap();
        let heartbeat = socket.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&heartbeat).unwrap(),
            json!({"op":"heartbeat"})
        );
        socket
            .close(Some(CloseFrame {
                code: 4001.into(),
                reason: SECRET.into(),
            }))
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(stream, |request: &Request, response| {
            assert_eq!(request.uri().query(), Some("resume=2"));
            Ok(response)
        })
        .await
        .unwrap();
        socket
            .send(Message::text(json!({"seq":3,"echo":SECRET}).to_string()))
            .await
            .unwrap();
        resumed.send(()).unwrap();
        let close = socket.next().await.unwrap().unwrap();
        assert!(matches!(close, Message::Close(Some(close)) if u16::from(close.code) == 4000));
        socket.flush().await.unwrap();
    });
    SocketServer {
        url,
        resumed: observed,
        task,
    }
}

async fn adapter(
    base: &str,
    socket: &str,
    cdn: &str,
) -> (tempfile::TempDir, Arc<SqliteStore>, Arc<dyn ChannelAdapter>) {
    let root = tempfile::tempdir().unwrap();
    let bytes = build(root.path(), socket, cdn).await;
    let store = Arc::new(
        SqliteStore::open(StorePaths {
            db: root.path().join("enco.db"),
            blobs: root.path().join("blobs"),
            artifacts: root.path().join("artifacts"),
        })
        .await
        .unwrap(),
    );
    let loaded = enco_wasm::WasmRuntime::new()
        .unwrap()
        .load(&bytes, &PluginSettings::default())
        .await
        .unwrap();
    let adapter = loaded
        .channel
        .unwrap()
        .open(
            &ChannelSettings {
                base_url: base.into(),
                secrets: [("token".into(), "ENCO_CHANNEL_FIXTURE_TOKEN".into())].into(),
            },
            GenerationId(32),
            store.clone(),
        )
        .await
        .unwrap();
    (root, store, adapter)
}

async fn build(root: &Path, socket: &str, cdn: &str) -> Vec<u8> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::os::unix::fs::symlink(
        repository.join("sdk").canonicalize().unwrap(),
        root.join("sdk"),
    )
    .unwrap();
    let directory = root.join("plugins/channel-socket");
    std::fs::create_dir_all(directory.join("src")).unwrap();
    let source = include_str!("fixtures/channel_socket.rs")
        .replace("SOCKET_ORIGIN", socket)
        .replace("CDN_ORIGIN", cdn);
    std::fs::write(directory.join("src/lib.rs"), source).unwrap();
    std::fs::write(
        directory.join("Cargo.toml"),
        r#"[package]
name = "channel-socket-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
crate-type = ["cdylib"]
[dependencies]
enco-sdk = { path = "../../sdk" }
serde_json = "1.0.151"
"#,
    )
    .unwrap();
    let target = repository.join("target/plugin-tests/channel-socket");
    let output = tokio::process::Command::new("cargo")
        .current_dir(&directory)
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "--offline",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(target.join("wasm32-wasip2/release/channel_socket_fixture.wasm")).unwrap()
}

async fn take_messages(
    readings: &mut mpsc::UnboundedReceiver<ChannelReading>,
    store: &dyn Store,
) -> oneshot::Receiver<()> {
    let mut first = None;
    let mut expected = 1;
    while expected <= 3 {
        let reading = tokio::time::timeout(Duration::from_secs(10), readings.recv())
            .await
            .unwrap()
            .unwrap();
        let ChannelReading::Batch {
            messages,
            protocol,
            accepted,
            acknowledged,
        } = reading
        else {
            panic!("unexpected checkpoint reset")
        };
        assert!(accepted.is_none());
        for message in messages {
            assert_eq!(message.key, expected.to_string());
            assert_eq!(protocol, Some(json!(expected)));
            let bytes = store.get_blob(&message.origin.call).await.unwrap();
            let record: MappingRecord = serde_json::from_slice(&bytes).unwrap();
            let state = if expected == 1 {
                Value::Null
            } else {
                json!((expected - 1).to_string())
            };
            assert_eq!(record.arguments["state"], state);
            expected += 1;
        }
        if first.is_none() {
            first = Some(acknowledged);
        }
    }
    first.unwrap()
}

async fn assert_records(root: &Path, store: &dyn Store) {
    let mut arrivals = Vec::new();
    for shard in std::fs::read_dir(root.join("blobs")).unwrap() {
        for file in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            assert!(
                !bytes
                    .windows(SECRET.len())
                    .any(|part| part == SECRET.as_bytes())
            );
            if let Ok(record) = serde_json::from_slice::<MappingRecord>(&bytes) {
                assert_eq!(record.generation, GenerationId(32));
                if record.export == "channel.received" {
                    arrivals.push(record.arguments["arrival"].clone());
                }
                if record.export == "channel.identified" {
                    let hash = serde_json::from_value(record.arguments["response"]["body"].clone())
                        .unwrap();
                    let bytes = store.get_blob(&hash).await.unwrap();
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes).unwrap()["echo"],
                        "[redacted:ENCO_CHANNEL_FIXTURE_TOKEN]"
                    );
                }
            }
        }
    }
    assert_eq!(arrivals.len(), 4);
    assert_eq!(
        arrivals
            .iter()
            .filter(|arrival| arrival.get("frame").is_some())
            .count(),
        3
    );
    assert!(
        arrivals
            .iter()
            .any(|arrival| arrival["closed"]["code"] == 4001
                && arrival["closed"]["reason"] == "[redacted:ENCO_CHANNEL_FIXTURE_TOKEN]")
    );
}
