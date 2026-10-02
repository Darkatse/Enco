#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;

use enco_core::*;
use enco_kernel::{Inspection, Runtime};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::json;
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
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("inspect")
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let inspection: Inspection = serde_json::from_slice(&output.stdout).unwrap();
    let CodeRef::Generation { id: generation } = inspection.provider else {
        panic!("provider must reference a generation");
    };
    let plugins = client.request(Command::PluginStatus {}).await.unwrap();
    let active = plugins
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "openai-compatible")
        .unwrap();
    assert_eq!(active["active"]["id"], json!(generation));
    let hash = active["active"]["artifact"].as_str().unwrap();
    let bytes = std::fs::read(
        daemon
            .root
            .path()
            .join(".data/artifacts")
            .join(format!("{hash}.wasm")),
    )
    .unwrap();
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
    // CLI deployment resolves a caller-relative path and uses the daemon's registry.
    std::fs::write(daemon.root.path().join("replacement.wasm"), bytes).unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["plugin", "deploy", "openai-compatible", "replacement.wasm"])
        .current_dir(daemon.root.path())
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deployed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(deployed["generation"]["id"], json!(generation));
    daemon.stop().await;
    daemon.restart().await;
    let mut client = daemon.connect().await;
    let plugins = client.request(Command::PluginStatus {}).await.unwrap();
    assert_eq!(
        plugins
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin["name"] == "openai-compatible")
            .unwrap()["active"],
        deployed["generation"]
    );
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["plugin", "rollback", "openai-compatible"])
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let restored: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(restored["id"], json!(generation));
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
        if plugin == "deepseek" {
            let config_path = daemon.root.path().join("config.toml");
            let config = std::fs::read_to_string(&config_path).unwrap();
            std::fs::write(
                &config_path,
                format!(
                    r#"{config}
[endpoint.alternate]
plugin = "openai-compatible"
base_url = {:?}
model = "alternate"
window_tokens = 128000
max_output_tokens = 4096

[profile.alternate]
reply = "alternate"
compaction = "alternate"
"#,
                    server.uri()
                ),
            )
            .unwrap();
            daemon.restart().await;
            let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
                .args(["profile", "main", "alternate"])
                .env("ENCO_HOME", daemon.root.path())
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
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
                    text: "continue with another plugin".into(),
                    event_id: EventId::new(),
                })
                .await
                .unwrap();
            finish(&mut client).await;
            let requests = server.received_requests().await.unwrap();
            let chat = requests
                .iter()
                .rev()
                .find(|request| request.url.path() == "/chat/completions")
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&chat.body).unwrap();
            assert!(
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|message| message.get("reasoning_content").is_none())
            );
            daemon.stop().await;
            daemon.restart().await;
            let mut client = daemon.connect().await;
            let status = client.request(Command::Status {}).await.unwrap();
            assert_eq!(status["sessions"][0]["session"]["profile"], "alternate");
            let attempt = entries
                .iter()
                .rev()
                .find_map(|entry| match entry.body {
                    EntryBody::AttemptStarted { attempt, .. } => Some(attempt),
                    _ => None,
                })
                .unwrap();
            let inspection: Inspection = serde_json::from_value(
                client
                    .request(Command::Inspect {
                        session: "main".into(),
                        attempt_id: Some(attempt),
                    })
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert!(
                inspection
                    .request
                    .messages
                    .iter()
                    .flat_map(|message| &message.parts)
                    .any(|part| matches!(part, Part::Extension(_)))
            );
            daemon.stop().await;
        }
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
    let runtime = WasmRuntime::new().unwrap();
    let settings = ProviderSettings {
        base_url: server.uri(),
        model: "embedding".into(),
        api_key_env: None,
        options: json!({}),
    };
    let provider = runtime
        .load(include_bytes!(env!("ENCO_FACTORY_OPENAI")), &json!({}))
        .await
        .unwrap()
        .embedding
        .unwrap();
    assert_eq!(
        provider
            .embed(&settings, None, vec!["one".into(), "two".into()])
            .await
            .unwrap(),
        vec![vec![1., 0.], vec![0., 1.]]
    );
    server.reset().await;
    Mock::given(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [
            { "embedding": [1., 0.] },
            { "index": 1, "embedding": [0.] },
        ] })))
        .mount(&server)
        .await;
    assert_eq!(
        provider
            .embed(&settings, None, vec!["one".into(), "two".into()])
            .await
            .unwrap_err()
            .code,
        code::PROVIDER_BAD_RESPONSE
    );
    let bad = ProviderSettings {
        options: json!({ "input": ["not the caller's input"] }),
        ..settings
    };
    assert_eq!(
        provider
            .embed(&bad, None, vec!["one".into()])
            .await
            .unwrap_err()
            .code,
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
    let text = format!("{}\n{}", "🌱".repeat(32767), "汉字".repeat(18000));
    let parts = telegram.split(&text);
    assert!(parts.iter().all(|s| s.chars().count() <= 32768));
    assert_eq!(parts.concat(), text);
    assert!(parts[0].ends_with('\n'));
    let markdown = "# Title\n\n**bold** and ||spoiler||\n\n```rust\nlet x = 1;\n```\n";
    let success = Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .and(wiremock::matchers::body_json(
            json!({"chat_id": "1", "rich_message": {"markdown": markdown}}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"message_id": 1}})),
        )
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    telegram.send("1", markdown).await.unwrap();
    drop(success);
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
        let guard = Mock::given(path("/bot42:test-secret/sendRichMessage"))
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
