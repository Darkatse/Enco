#![expect(
    clippy::unwrap_used,
    reason = "fixture helpers fail the test by panicking"
)]
use crate::protocol;
use crate::support;

use enco_core::{EntryBody, EventId, RunEnd, code};
use protocol::Command;
use serde_json::{Value, json};
use support::*;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

/// Serves embeddings and completions, except the `rejected` model and tool requests to `no-tools`.
async fn model_service() -> MockServer {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if body["model"] == "rejected"
                || (body["model"] == "no-tools" && body.get("tools").is_some())
            {
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error": {"message": "unsupported request"}}))
            } else {
                response(json!({"role": "assistant", "content": "OK"}), "stop")
            }
        })
        .mount(&server)
        .await;
    server
}

/// The fixture's `config.toml` as a document; tests edit tables, never text.
fn configuration(server: &MockServer) -> Value {
    toml::from_str(&Daemon::config(server, "openai-compatible")).unwrap()
}

fn write(daemon: &Daemon, document: &Value) {
    std::fs::write(
        daemon.root.path().join("config.toml"),
        toml::to_string(document).unwrap(),
    )
    .unwrap();
}

/// Writes the intent and applies it, as an owner editing `config.toml` would.
async fn apply(daemon: &Daemon, document: &Value) -> Result<Value, String> {
    write(daemon, document);
    daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .map(|value| value["registered"].clone())
        .map_err(|error| error.to_string())
}

async fn config_status(daemon: &Daemon) -> Value {
    daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap()["configuration"]
        .clone()
}

#[tokio::test]
async fn first_registration_adopts_config_toml_without_model_requests() {
    let server = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let status = config_status(&daemon).await;
    assert_eq!(status["running"], status["registered"]);
    assert!(status["running"]["registration"].is_number());
    assert!(server.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn each_owner_rejects_its_invalid_table_without_registering() {
    let server = MockServer::start().await;
    let original = configuration(&server);
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let before = config_status(&daemon).await;
    let mut cases = vec![];
    let mut invalid = original.clone();
    invalid["owner"] = json!({});
    cases.push((invalid, vec!["[owner]", "timezone"]));
    let mut invalid = original.clone();
    invalid["plugin"] = json!({"web": {"unsupported": true}});
    cases.push((invalid, vec!["[plugin]"]));
    for (endpoint, field, table) in [
        ("chat", "window_tokens", "[profile.default]"),
        ("embed", "dimensions", "[memory]"),
    ] {
        let mut invalid = original.clone();
        invalid["endpoint"][endpoint]
            .as_object_mut()
            .unwrap()
            .remove(field);
        cases.push((invalid, vec![table, endpoint, field]));
    }
    for (invalid, expected) in cases {
        let error = apply(&daemon, &invalid).await.unwrap_err();
        assert!(expected.iter().all(|part| error.contains(part)), "{error}");
    }
    assert_eq!(
        config_status(&daemon).await["registered"],
        before["registered"]
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn reply_endpoints_the_service_rejects_are_not_registered() {
    let server = model_service().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let before = config_status(&daemon).await;
    for model in ["rejected", "no-tools"] {
        let mut candidate = configuration(&server);
        candidate["endpoint"]["chat"]["model"] = model.into();
        let error = apply(&daemon, &candidate).await.unwrap_err();
        assert!(
            error.contains("profile default.reply")
                && error.contains("endpoint.chat")
                && error.contains("provider."),
            "{error}"
        );
    }
    assert_eq!(
        config_status(&daemon).await["registered"],
        before["registered"]
    );
    daemon.stop().await;
}

#[tokio::test]
async fn applying_calls_only_the_changed_endpoint_once() {
    let server = model_service().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let mut candidate = configuration(&server);
    candidate["endpoint"]["cheap"] = candidate["endpoint"]["chat"].clone();
    candidate["endpoint"]["cheap"]["model"] = "cheap".into();
    candidate["profile"]["default"]["compaction"] = json!(["cheap"]);
    let applied = apply(&daemon, &candidate).await.unwrap();
    assert_eq!(
        applied["probes"],
        json!([{"user": "profile default.compaction", "endpoint": "cheap"}])
    );
    // Startup made no requests, so the reply and embedding endpoints were not called.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/chat/completions");
    let probe: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(probe["model"], "cheap");
    assert_eq!(probe["max_tokens"], 8192);
    assert!(probe.get("tools").is_none());
    let again = apply(&daemon, &candidate).await.unwrap();
    assert!(again["registration"].is_null());
    assert_eq!(
        config_status(&daemon).await["running"]["generation"],
        applied["configuration"]["id"]
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    daemon.stop().await;
}

#[tokio::test]
async fn restart_restores_the_active_generation_whatever_config_toml_holds() {
    let server = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let registered = config_status(&daemon).await;
    let mut unapplied = configuration(&server);
    unapplied["owner"]["timezone"] = "America/New_York".into();
    for (text, unreadable) in [
        (toml::to_string(&unapplied).unwrap(), false),
        ("[broken".to_owned(), true),
    ] {
        daemon.stop().await;
        std::fs::write(daemon.root.path().join("config.toml"), text).unwrap();
        daemon.restart().await;
        let status = config_status(&daemon).await;
        assert_eq!(status["running"], registered["running"]);
        assert_eq!(status["registered"], registered["registered"]);
        let status = daemon
            .connect()
            .await
            .request(Command::Status {
                output: protocol::Output::Json,
            })
            .await
            .unwrap();
        let comparison = &status["configuration"]["workspace"];
        assert_eq!(
            comparison["state"],
            if unreadable { "unreadable" } else { "changed" }
        );
        if unreadable {
            assert_eq!(
                comparison["path"],
                json!(daemon.root.path().join("config.toml"))
            );
            assert_eq!(comparison["problem"]["problem"], "toml");
        }
    }
    daemon.stop().await;
}

#[tokio::test]
async fn rewind_restores_earlier_intent_and_the_running_document_after_owner_rollback() {
    let server = model_service().await;
    let mut original = configuration(&server);
    // A second embedding endpoint lets the last edit change only the memory table.
    original["endpoint"]["spare"] = original["endpoint"]["embed"].clone();
    let mut daemon = Daemon::start_config(&toml::to_string(&original).unwrap()).await;
    let root = daemon.root.path().to_path_buf();
    let first = config_status(&daemon).await["running"]["registration"].clone();
    let mut changed = original.clone();
    changed["owner"]["timezone"] = "America/New_York".into();
    let second = apply(&daemon, &changed).await.unwrap()["configuration"].clone();
    daemon
        .connect()
        .await
        .request(Command::Rewind {
            output: protocol::Output::Json,
            arguments: enco_host::development::RewindArguments {
                registration: serde_json::from_value(first).unwrap(),
                paths: Some(vec!["config.toml".parse().unwrap()]),
            },
        })
        .await
        .unwrap();
    let undo = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap();
    let third = &undo["registered"]["configuration"];
    assert!(third["id"].as_u64().unwrap() > second["id"].as_u64().unwrap());
    assert_eq!(third["document"], original);
    daemon
        .connect()
        .await
        .request(Command::Rollback {})
        .await
        .unwrap();
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        status["configuration"]["running"]["generation"],
        second["id"]
    );
    assert_eq!(
        status["configuration"]["registered"]["generation"],
        third["id"]
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    // Start from the running document, so editing one table cannot re-register the undone one.
    daemon
        .connect()
        .await
        .request(Command::Rewind {
            output: protocol::Output::Json,
            arguments: enco_host::development::RewindArguments {
                registration: serde_json::from_value(second["registration"].clone()).unwrap(),
                paths: Some(vec!["config.toml".parse().unwrap()]),
            },
        })
        .await
        .unwrap();
    let mut edited: Value =
        toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap();
    assert_eq!(edited, second["document"]);
    edited["memory"]["embedding"] = "spare".into();
    let fourth = apply(&daemon, &edited).await.unwrap()["configuration"].clone();
    assert_eq!(fourth["document"], edited);
    assert_ne!(fourth["document"]["owner"], third["document"]["owner"]);
    daemon.stop().await;
}

#[tokio::test]
async fn telegram_follows_a_new_api_base_after_the_in_flight_delivery_settles() {
    switch_bot(false).await;
}

#[tokio::test]
async fn telegram_follows_a_new_api_base_even_when_the_old_settlement_fails() {
    switch_bot(true).await;
}

/// Answers a pending poll with one owner message; returns the reply's send and the next poll.
async fn message(
    bot: &mut bot::Bot,
    poll: bot::Request,
    update_id: u64,
    text: &str,
) -> (bot::Request, bot::Request) {
    assert!(poll.path.ends_with("getUpdates"));
    poll.reply
        .send(json!({"ok": true, "result": [{"update_id": update_id, "message": {"chat": {"id": 7, "type": "private"}, "from": {"id": 9}, "text": text}}]}))
        .unwrap();
    let a = bot.next().await;
    let b = bot.next().await;
    let (send, poll) = if a.path.ends_with("sendRichMessage") {
        (a, b)
    } else {
        (b, a)
    };
    assert!(send.path.ends_with("sendRichMessage") && poll.path.ends_with("getUpdates"));
    assert_eq!(poll.body["offset"], update_id + 1);
    (send, poll)
}

/// Moves the channel to a second bot while its reply to the first message is still in flight.
async fn switch_bot(fail_settlement: bool) {
    let service = MockServer::start().await;
    embeddings(&service).await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({"role": "assistant", "content": "live reply"}),
            "stop",
        ))
        .mount(&service)
        .await;
    let mut first = bot::Bot::start().await;
    let mut second = bot::Bot::start().await;
    let mut document = configuration(&service);
    document["telegram"] =
        json!({"token_env": "ENCO_TEST_TELEGRAM_TOKEN", "owner_user_id": 9, "api_base": first.url});
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let poll = first.next().await;
    let (send, poll) = message(&mut first, poll, 10, "hello").await;
    let sql = rusqlite::Connection::open(daemon.root.path().join(".data/enco.db")).unwrap();
    if fail_settlement {
        // Reject only the old owner's `ok` settlement; recording an interruption still works.
        sql.execute_batch("CREATE TRIGGER reject_settlement BEFORE INSERT ON deliveries WHEN json_extract(NEW.body, '$.outcome.kind') = 'ok' BEGIN SELECT RAISE(ABORT, 'injected settlement failure'); END;").unwrap();
    }
    document["telegram"]["api_base"] = second.url.clone().into();
    apply(&daemon, &document).await.unwrap();
    // The cancelled poll proves shutdown has begun; the send still owns its side effect.
    tokio::time::timeout(std::time::Duration::from_secs(15), poll.closed)
        .await
        .unwrap()
        .unwrap();
    send.reply
        .send(json!({"ok": true, "result": {"message_id": 1}}))
        .unwrap();
    // The next connection polls only after the old owner has joined, and it recovers first.
    let poll = second.next().await;
    assert_eq!(poll.body["offset"], 11);
    if fail_settlement {
        sql.execute_batch("DROP TRIGGER reject_settlement").unwrap();
    }
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    let channel = &status["node"]["channels"][0];
    assert_eq!(channel["running"], true);
    let failures = channel["recent_failures"].as_array().unwrap();
    if fail_settlement {
        // The unsettled send is recovered as interrupted and never resent.
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["outcome"]["kind"], "unknown");
        assert_eq!(failures[0]["outcome"]["failure"]["code"], code::INTERRUPTED);
    } else {
        // The old owner settled its own send, so the next connection had nothing to recover.
        assert!(failures.is_empty(), "{failures:?}");
    }
    let (send, _poll) = message(&mut second, poll, 11, "hello again").await;
    assert!(
        send.body["rich_message"]["markdown"]
            .as_str()
            .unwrap()
            .contains("live reply")
    );
    send.reply
        .send(json!({"ok": true, "result": {"message_id": 2}}))
        .unwrap();
    daemon.stop().await;
    first.shutdown().await;
    second.shutdown().await;
}

#[tokio::test]
async fn fresh_home_rejects_invalid_configuration_before_serving() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".data")).unwrap();
    let mut document = configuration(&server);
    document["endpoint"]["chat"]
        .as_object_mut()
        .unwrap()
        .remove("window_tokens");
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    initialize(root.path()).await;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
            .arg("serve")
            .env("ENCO_HOME", root.path())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("[profile.default]")
            && error.contains("endpoint chat")
            && error.contains("window_tokens"),
        "{error}"
    );
    assert!(!root.path().join(".data/enco.sock").exists());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn restart_restores_registered_configuration_without_credentials() {
    let server = MockServer::start().await;
    let mut document = configuration(&server);
    document["telegram"] = json!({"token_env": "ENCO_TEST_TELEGRAM_TOKEN", "owner_user_id": 9, "api_base": server.uri()});
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let before = config_status(&daemon).await["running"].clone();
    daemon.stop().await;
    daemon
        .restart_without(&["ENCO_TEST_API_KEY", "ENCO_TEST_TELEGRAM_TOKEN"])
        .await;
    let mut client = daemon.connect().await;
    let status = client
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(status["configuration"]["running"], before);
    assert!(
        status["configuration"]["invalid"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        status["node"]["channels"][0]["stopped"]
            .as_str()
            .unwrap()
            .contains("ENCO_TEST_TELEGRAM_TOKEN")
    );
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            images: Vec::new(),
            session: "main".into(),
            text: "hello".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let entries = finish(&mut client).await;
    assert!(
        matches!(&entries.last().unwrap().body, EntryBody::RunEnded { end: RunEnd::Failed { failure }, .. } if failure.code == code::CREDENTIAL_UNAVAILABLE && failure.message.contains("ENCO_TEST_API_KEY"))
    );
    daemon.stop().await;
}
