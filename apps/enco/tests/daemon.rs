#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;

use enco_core::*;
use enco_kernel::Provider;
use enco_wasm::{ProviderSettings, WasmEngine, WasmProvider};
use protocol::Command;
use serde_json::json;
use std::sync::Arc;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

mod support;

use support::*;

#[tokio::test]
async fn cli_to_wasm_to_http_records_a_reply_and_its_artifact() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({ "role": "assistant", "content": "hello" }),
            "stop",
        ))
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["send", "hello"])
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let entries = finish(&mut client).await;
    assert!(entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::AttemptSettled { result: AttemptResult::Completed { message, .. }, .. }
            if message.joined_text() == "hello"
    )));
    let hash = entries
        .iter()
        .find_map(|e| match e.body {
            EntryBody::AttemptStarted {
                provider: CodeRef::Wasm { artifact },
                ..
            } => Some(artifact),
            _ => None,
        })
        .unwrap();
    let text = hash.to_string();
    let bytes = std::fs::read(
        daemon
            .root
            .path()
            .join(".data/blobs")
            .join(&text[..2])
            .join(&text),
    )
    .unwrap();
    assert_eq!(ContentHash::of(&bytes), hash);
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("log")
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let log: Vec<Entry> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(log, entries);
    let status = client.request(Command::Status {}).await.unwrap();
    let id = status["sessions"][0]["session"]["id"].as_str().unwrap();
    let by_id = client
        .request(Command::Log {
            session: id.into(),
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(by_id, json!(log));
    daemon.stop().await;
}

#[tokio::test]
async fn both_plugins_preserve_tool_ids_and_provider_extensions_across_rounds() {
    for plugin in ["openai-compatible", "deepseek"] {
        let server = MockServer::start().await;
        embeddings(&server).await;
        Mock::given(path("/chat/completions"))
            .respond_with(|request: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let messages = body["messages"].as_array().unwrap();
                if messages.iter().any(|message| message["role"] == "tool") {
                    return response(json!({ "role": "assistant", "content": "done" }), "stop");
                }
                let mut message = tool_message(
                    "service-call-1",
                    "fs_write",
                    json!({ "path": "written.txt", "content": "from wasm" }),
                );
                message["reasoning_content"] = json!("write the requested file");
                response(message, "tool_calls")
            })
            .mount(&server)
            .await;
        let mut daemon = Daemon::start(&server, plugin).await;
        let mut client = daemon.connect().await;
        client
            .request(Command::Subscribe {
                session: "main".into(),
            })
            .await
            .unwrap();
        client
            .send(Command::Send {
                session: "main".into(),
                text: "write a file".into(),
                event_id: EventId::new(),
            })
            .await
            .unwrap();
        let entries = finish(&mut client).await;
        assert!(matches!(
            entries.last().unwrap().body,
            EntryBody::RunEnded {
                end: RunEnd::Completed,
                ..
            }
        ));
        assert_eq!(
            std::fs::read_to_string(daemon.root.path().join("workspace/written.txt")).unwrap(),
            "from wasm"
        );
        let requests = server.received_requests().await.unwrap();
        let chat: Vec<_> = requests
            .iter()
            .filter(|r| r.url.path() == "/chat/completions")
            .collect();
        assert_eq!(chat.len(), 2);
        let body: serde_json::Value = serde_json::from_slice(&chat[1].body).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert!(
            messages
                .iter()
                .any(|m| m["role"] == "tool" && m["tool_call_id"] == "service-call-1")
        );
        assert!(
            messages.iter().any(|m| m["role"] == "assistant"
                && m["reasoning_content"] == "write the requested file")
        );
        daemon.stop().await;
    }
}

#[tokio::test]
async fn wasm_embedding_preserves_batch_order_and_rejects_reserved_options() {
    let server = MockServer::start().await;
    Mock::given(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [
                    { "index": 1, "embedding": [0., 1.] },
                    { "embedding": [1., 0.] },
                ] })))
        .mount(&server)
        .await;
    let engine = Arc::new(WasmEngine::new().unwrap());
    let settings = ProviderSettings {
        base_url: server.uri(),
        model: "embedding".into(),
        api_key: None,
        options: json!({}),
    };
    let provider = WasmProvider::new(
        engine.clone(),
        include_bytes!(env!("ENCO_FACTORY_OPENAI")),
        settings.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        provider
            .embed(vec!["one".into(), "two".into()])
            .await
            .unwrap(),
        vec![vec![1., 0.], vec![0., 1.]]
    );
    let bad = WasmProvider::new(
        engine,
        include_bytes!(env!("ENCO_FACTORY_OPENAI")),
        ProviderSettings {
            options: json!({ "input": ["not the caller's input"] }),
            ..settings
        },
    )
    .await
    .unwrap();
    assert_eq!(
        bad.embed(vec!["one".into()]).await.unwrap_err().code,
        code::PROVIDER_BAD_REQUEST
    );
}

#[tokio::test]
async fn telegram_protocol_owns_commands_text_limits_and_delivery_evidence() {
    use enco_host::{
        Telegram,
        channel::{Action, Adapter},
    };
    let server = MockServer::start().await;
    let telegram = Telegram::new("42:test-secret".into(), &server.uri()).unwrap();
    Mock::given(path("/bot42:test-secret/getUpdates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "result": [
                {"update_id": 8, "message": {"chat": {"id": 1, "type": "private"}, "from": {"id": 2}, "text": "/session work"}},
                {"update_id": 9, "message": {"chat": {"id": 1, "type": "group"}, "from": {"id": 3}, "text": "ignored by owner"}},
                {"update_id": 10, "message": {"chat": {"id": 1, "type": "private"}, "from": {"id": 2}}}
            ]
        }))).mount(&server).await;
    let updates = telegram.poll(telegram.initial_state()).await.unwrap();
    assert!(
        matches!(&updates[0].input.as_ref().unwrap().action, Some(Action::SelectSession(name)) if name == "work")
    );
    assert!(!updates[1].input.as_ref().unwrap().direct);
    assert!(updates[2].input.as_ref().unwrap().action.is_none());
    assert_eq!(updates[2].protocol["offset"], 11);
    let text = format!("{}\n{}", "🌱".repeat(2047), "汉字".repeat(2500));
    let parts = telegram.split(&text);
    assert!(parts.iter().all(|s| s.encode_utf16().count() <= 4096));
    assert_eq!(parts.concat(), text);
    for (status, body, unknown, retry, fatal) in [
        (
            429,
            json!({"ok": false, "error_code": 429, "parameters": {"retry_after": 2}}),
            false,
            true,
            false,
        ),
        (
            403,
            json!({"ok": false, "error_code": 403}),
            false,
            false,
            false,
        ),
        (
            401,
            json!({"ok": false, "error_code": 401}),
            false,
            false,
            true,
        ),
        (
            500,
            json!({"ok": false, "error_code": 500}),
            true,
            true,
            false,
        ),
        (
            502,
            json!("proxy failed after forwarding"),
            true,
            true,
            false,
        ),
        (200, json!({"ok": true, "result": {}}), true, false, false),
    ] {
        let guard = Mock::given(path("/bot42:test-secret/sendMessage"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount_as_scoped(&server)
            .await;
        let error = telegram.send("1", "hello").await.unwrap_err();
        assert_eq!(
            (error.unknown, error.failure.retryable, error.fatal),
            (unknown, retry, fatal)
        );
        assert!(!error.to_string().contains("test-secret"));
        if status == 429 {
            assert_eq!(error.retry_after, Some(std::time::Duration::from_secs(2)));
        }
        drop(guard);
    }
}
