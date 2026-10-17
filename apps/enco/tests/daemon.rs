#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;

use enco_core::*;
use enco_kernel::{Inspection, Runtime};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, path},
};

mod support;

use support::*;

/// A service that embeds and answers every chat request with "hello".
async fn hello_service() -> MockServer {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({ "role": "assistant", "content": "hello" }),
            "stop",
        ))
        .mount(&server)
        .await;
    server
}

/// Sends `text` to `session` and returns the entries of the Run it starts.
async fn submit(daemon: &Daemon, session: &str, text: &str) -> Vec<Entry> {
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: session.into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            session: session.into(),
            text: text.into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    finish(&mut client).await
}

#[tokio::test]
async fn cli_to_wasm_to_http_records_a_reply() {
    let server = hello_service().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["send", "--wait", "hello"])
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let waited: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = finish(&mut client).await;
    let ended = entries.last().unwrap();
    let EntryBody::RunEnded { run, end } = &ended.body else {
        panic!("Run did not end");
    };
    assert_eq!(waited["run"], json!(run));
    assert_eq!(waited["end"], json!(end));
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
    let plugins = client.request(Command::Status {}).await.unwrap()["plugins"].clone();
    let active = plugins
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "openai-compatible")
        .unwrap();
    assert_eq!(active["active"]["id"], json!(inspection.provider));
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
async fn cli_deploys_a_caller_relative_artifact_that_survives_restart_and_rolls_back() {
    let server = hello_service().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let plugins = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap()["plugins"]
        .clone();
    let active = &plugins
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "openai-compatible")
        .unwrap()["active"];
    let generation = active["id"].clone();
    let hash = active["artifact"].as_str().unwrap();
    // The registry records the active code as a content-addressed artifact.
    let bytes = std::fs::read(
        daemon
            .root
            .path()
            .join(".data/artifacts")
            .join(format!("{hash}.wasm")),
    )
    .unwrap();
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
    let revision = deployed["registration"]["revision"].as_str().unwrap();
    assert_eq!(
        git(
            daemon.root.path(),
            &["rev-parse", &format!("{revision}^{{tree}}")]
        )
        .await,
        git(
            daemon.root.path(),
            &["rev-parse", &format!("{revision}^^{{tree}}")]
        )
        .await
    );
    let unchanged = daemon
        .connect()
        .await
        .request(Command::Apply {})
        .await
        .unwrap();
    assert!(unchanged["registered"].is_null());
    assert_eq!(
        git(daemon.root.path(), &["rev-parse", "enco/registered"]).await,
        revision
    );

    assert_ne!(deployed["generations"][0]["id"], generation);
    daemon.stop().await;
    daemon.restart().await;
    let mut client = daemon.connect().await;
    let plugins = client.request(Command::Status {}).await.unwrap()["plugins"].clone();
    assert_eq!(
        plugins
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin["name"] == "openai-compatible")
            .unwrap()["active"],
        deployed["generations"][0]
    );
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("rollback")
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let restored: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(restored["plugins"][0]["active"]["generation"], generation);
    daemon.stop().await;
}

#[tokio::test]
async fn credentials_stay_at_the_service_origin_and_out_of_recorded_results() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "deepseek").await;
    for (session, reply, completed) in [
        (
            "same-origin",
            ResponseTemplate::new(307).insert_header("Location", format!("{}/reply", server.uri())),
            true,
        ),
        (
            "cross-origin",
            ResponseTemplate::new(302).insert_header("Location", format!("{}/reply", other.uri())),
            false,
        ),
        (
            "service-error",
            ResponseTemplate::new(401).set_body_string("rejected enco-test-secret"),
            false,
        ),
    ] {
        server.reset().await;
        Mock::given(path("/chat/completions"))
            .and(header("Authorization", "Bearer enco-test-secret"))
            .respond_with(reply)
            .expect(1..)
            .mount(&server)
            .await;
        Mock::given(path("/reply"))
            .and(header("Authorization", "Bearer enco-test-secret"))
            .respond_with(support::response(
                json!({
                    "role": "assistant", "content": "echo enco-test-secret"
                }),
                "stop",
            ))
            .mount(&server)
            .await;
        let mut client = daemon.connect().await;
        client
            .request(Command::Subscribe {
                session: session.into(),
            })
            .await
            .unwrap();
        client
            .send(Command::Send {
                session: session.into(),
                text: "hello".into(),
                event_id: EventId::new(),
            })
            .await
            .unwrap();
        let entries = finish(&mut client).await;
        let inspection: Inspection = serde_json::from_value(
            client
                .request(Command::Inspect {
                    session: session.into(),
                    attempt_id: None,
                })
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            matches!(inspection.result, Some(AttemptResult::Completed { .. })),
            completed
        );
        let recorded = serde_json::to_string(&(entries, inspection)).unwrap();
        assert!(!recorded.contains("enco-test-secret"));
        if session != "cross-origin" {
            assert!(recorded.contains("[redacted:ENCO_TEST_API_KEY]"));
        }
        server.verify().await;
    }
    assert!(other.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn unusable_completions_fail_the_run_and_stay_out_of_later_requests() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    let responses = [
        // A response cut off upstream may not say how it ended.
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "The sky is" },
                "finish_reason": null,
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0 },
        })),
        // A reasoning model can spend the whole output limit before writing any text.
        response(
            json!({ "role": "assistant", "content": "", "reasoning_content": "unfinished thought" }),
            "length",
        ),
        response(json!({ "role": "assistant", "content": "hello" }), "stop"),
    ];
    for template in responses {
        Mock::given(path("/chat/completions"))
            .respond_with(template)
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let mut ends = vec![];
    for text in ["first", "second", "third"] {
        client
            .send(Command::Send {
                session: "main".into(),
                text: text.into(),
                event_id: EventId::new(),
            })
            .await
            .unwrap();
        ends.push(finish(&mut client).await.pop().unwrap().body);
    }
    for end in &ends[..2] {
        assert!(matches!(
            end,
            EntryBody::RunEnded { end: RunEnd::Failed { failure }, .. }
                if failure.code == code::PROVIDER_BAD_RESPONSE
        ));
    }
    assert!(matches!(
        ends[2],
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    let requests = server.received_requests().await.unwrap();
    let chat: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/chat/completions")
        .collect();
    assert_eq!(chat.len(), 3);
    // Neither unusable completion becomes part of the conversation.
    let body: serde_json::Value = serde_json::from_slice(&chat[2].body).unwrap();
    assert!(
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["role"] != "assistant")
    );
    daemon.stop().await;
}

#[tokio::test]
async fn both_plugins_preserve_tool_ids_and_provider_extensions_across_rounds() {
    for plugin in ["openai-compatible", "deepseek"] {
        let (server, mut daemon, entries) = write_a_file(plugin).await;
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

/// Runs one write through `plugin`, whose service proposes the tool call with reasoning.
async fn write_a_file(plugin: &str) -> (MockServer, Daemon, Vec<Entry>) {
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
    let daemon = Daemon::start(&server, plugin).await;
    let entries = submit(&daemon, "main", "write a file").await;
    (server, daemon, entries)
}

#[tokio::test]
async fn another_plugin_receives_no_foreign_extensions_while_the_record_keeps_them() {
    let (server, mut daemon, entries) = write_a_file("deepseek").await;
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
reply = ["alternate"]
compaction = ["alternate"]
"#,
            server.uri()
        ),
    )
    .unwrap();
    daemon
        .connect()
        .await
        .request(Command::Apply {})
        .await
        .unwrap();
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
    submit(&daemon, "main", "continue with another plugin").await;
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
        .load(
            include_bytes!(env!("ENCO_FACTORY_OPENAI_COMPATIBLE")),
            &enco_core::PluginSettings::default(),
        )
        .await
        .unwrap()
        .embedding
        .unwrap();
    assert_eq!(
        provider
            .embed(&settings, vec!["one".into(), "two".into()])
            .await
            .unwrap(),
        vec![vec![1., 0.], vec![0., 1.]]
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| !request.headers.contains_key("authorization"))
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
            .embed(&settings, vec!["one".into(), "two".into()])
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
            .embed(&bad, vec!["one".into()])
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

#[tokio::test]
async fn completion_imports_are_rejected_before_linking_a_plugin() {
    let runtime = WasmRuntime::new().unwrap();
    let error = runtime
        .load(
            br#"(component (import "enco:plugin/completion@0.4.0" (instance)))"#,
            &PluginSettings::default(),
        )
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("Attempts by the kernel"));
}
