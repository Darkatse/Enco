#![expect(
    clippy::unwrap_used,
    reason = "fixture helpers fail the test by panicking"
)]
#[path = "../src/client.rs"]
mod client;
#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::{EntryBody, EventId, GenerationId, RunEnd, code};
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
        .request(Command::ConfigApply { generation: None })
        .await
        .map_err(|error| error.to_string())
}

async fn config_status(daemon: &Daemon) -> Value {
    daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap()["config"]
        .clone()
}

#[tokio::test]
async fn first_registration_adopts_config_toml_without_model_requests() {
    let server = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let status = config_status(&daemon).await;
    let generations = status["generations"].as_array().unwrap();
    assert_eq!(generations.len(), 1);
    assert_eq!(status["active"], generations[0][0]);
    assert_eq!(status["divergence"]["kind"], "none");
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
    invalid["plugin"] = json!({"web": {"config": {}}});
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
        config_status(&daemon).await["generations"],
        before["generations"]
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
                && error.contains("endpoint chat")
                && error.contains("provider."),
            "{error}"
        );
    }
    assert_eq!(
        config_status(&daemon).await["generations"],
        before["generations"]
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
    assert!(probe.get("tools").is_none());
    let again = apply(&daemon, &candidate).await.unwrap();
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["generation"], applied["generation"]);
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
        assert!(
            daemon.startup_log.contains("unapplied"),
            "{}",
            daemon.startup_log
        );
        let status = config_status(&daemon).await;
        assert_eq!(status["active"], registered["active"]);
        assert_eq!(status["generations"], registered["generations"]);
        assert_eq!(status["divergence"]["kind"], "unapplied");
        let reason = &status["divergence"]["reason"];
        if unreadable {
            assert!(reason.as_str().unwrap().contains("config.toml"), "{reason}");
        } else {
            assert!(reason.is_null(), "{reason}");
        }
    }
    daemon.stop().await;
}

#[tokio::test]
async fn undo_registers_earlier_content_and_rollback_reactivates_a_generation() {
    let server = MockServer::start().await;
    let original = configuration(&server);
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let first = config_status(&daemon).await["active"].as_u64().unwrap();
    let mut changed = original.clone();
    changed["owner"]["timezone"] = "America/New_York".into();
    let second = apply(&daemon, &changed).await.unwrap()["generation"]
        .as_u64()
        .unwrap();
    let undo = daemon
        .connect()
        .await
        .request(Command::ConfigApply {
            generation: Some(GenerationId(first)),
        })
        .await
        .unwrap();
    let third = undo["generation"].as_u64().unwrap();
    assert!(third > second);
    let path = daemon.root.path().join("config.toml");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        toml::to_string(&changed).unwrap()
    );
    let status = config_status(&daemon).await;
    assert_eq!(status["active"], third);
    assert_eq!(status["divergence"]["kind"], "superseded");
    assert_eq!(status["divergence"]["generation"], second);
    assert!(
        status["divergence"]["reason"]
            .as_str()
            .unwrap()
            .contains(&format!("re-registered the content of {first}"))
    );
    // The new generation holds the first one's document.
    write(&daemon, &original);
    assert_eq!(config_status(&daemon).await["divergence"]["kind"], "none");

    daemon
        .connect()
        .await
        .request(Command::ConfigRollback {})
        .await
        .unwrap();
    let status = config_status(&daemon).await;
    assert_eq!(status["active"], second);
    assert_eq!(status["divergence"]["kind"], "superseded");
    assert_eq!(status["divergence"]["generation"], third);
    assert!(
        status["divergence"]["reason"]
            .as_str()
            .unwrap()
            .contains(&format!(
                "mechanical rollback activated earlier configuration {second}"
            ))
    );
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
        .request(Command::Status {})
        .await
        .unwrap();
    let channel = &status["channels"][0];
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
    let before = config_status(&daemon).await["active"].clone();
    daemon.stop().await;
    daemon
        .restart_without(&["ENCO_TEST_API_KEY", "ENCO_TEST_TELEGRAM_TOKEN"])
        .await;
    let mut client = daemon.connect().await;
    let status = client.request(Command::Status {}).await.unwrap();
    assert_eq!(status["config"]["active"], before);
    assert!(status["config"]["invalid"].as_array().unwrap().is_empty());
    assert!(
        status["channels"][0]["stopped"]
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
