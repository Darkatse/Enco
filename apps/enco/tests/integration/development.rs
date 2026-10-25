#![expect(clippy::unwrap_used, reason = "test failures retain their context")]
mod acceptance;
use crate::protocol;
mod documents;
mod manual;
mod sdk;
use crate::support;

use enco_core::{
    CallId, Entry, EntryBody, EventId, GenerationStatus, Outcome, PluginSettings, SessionId,
    Settlement, code,
};
use enco_host::{SqliteStore, StorePaths};
use enco_kernel::{CallContext, Runtime, Store};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use support::{Daemon, embeddings, finish, response, running_generation, tool_message};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::{Layer, layer::SubscriberExt};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

/// Observe the guest's own entry marker before cancelling its CPU-bound call.
struct Entered(Arc<Notify>);
impl<S: tracing::Subscriber> Layer<S> for Entered {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() == "plugin" {
            self.0.notify_one();
        }
    }
}

#[tokio::test]
async fn cancelling_a_source_built_computing_plugin_settles_unknown_and_joins() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let server = MockServer::start().await;
    std::fs::write(
        home.join("config.toml"),
        Daemon::config(&server, "openai-compatible"),
    )
    .unwrap();
    std::fs::create_dir(home.join(".data")).unwrap();
    support::build_target(home, "spin");
    support::copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/test-fixture"),
        &home.join("plugins/fixture"),
    );
    let mut daemon = Daemon::start_home(root).await;
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    let artifact = running_generation(&status["plugins"]["fixture"])["artifact"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let home = daemon.root.path();
    let store = SqliteStore::open(StorePaths {
        db: home.join(".data/enco.db"),
        blobs: home.join(".data/blobs"),
        artifacts: home.join(".data/artifacts"),
    })
    .await
    .unwrap();
    let bytes = store.artifact(&artifact).await.unwrap();
    daemon.stop().await;
    let runtime = WasmRuntime::new().unwrap();
    let tools = runtime
        .load(&bytes, &PluginSettings::default())
        .await
        .unwrap()
        .tools
        .unwrap();
    let spec = tools
        .specs
        .iter()
        .find(|spec| spec.name == "fixture_spin")
        .unwrap()
        .clone();
    let entered = Arc::new(Notify::new());
    let subscriber = tracing_subscriber::registry().with(Entered(entered.clone()));
    let cancel = CancellationToken::new();
    let context = CallContext {
        session: SessionId::new(),
        call: CallId::new(),
        cancel: cancel.clone(),
        result_budget: 1024,
    };
    let invoked = tokio::spawn(
        async move { tools.adapter.call(context, &spec, Default::default()).await }
            .with_subscriber(subscriber),
    );
    tokio::time::timeout(Duration::from_secs(8), entered.notified())
        .await
        .unwrap();
    cancel.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(8), invoked)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, Outcome::Unknown { failure } if failure.code == code::CANCELLED));
}

async fn apply(daemon: &Daemon) -> Value {
    daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap()
}

fn write_config(daemon: &Daemon, document: &Value) {
    std::fs::write(
        daemon.root.path().join("config.toml"),
        toml::to_string(document).unwrap(),
    )
    .unwrap();
}

const REGISTER: &str = "register changes";
const RENAME: &str = "rename alpha to beta";
const HISTORY: &str = "review the unregistered files";

async fn echo_service() -> MockServer {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(echo_reply)
        .mount(&server)
        .await;
    server
}

fn echo_reply(request: &wiremock::Request) -> ResponseTemplate {
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    let messages = body["messages"].as_array().unwrap();
    if messages.iter().any(|message| message["role"] == "tool") {
        return response(json!({"role": "assistant", "content": "done"}), "stop");
    }
    if messages
        .iter()
        .any(|message| message["content"] == REGISTER)
    {
        return response(
            tool_message(
                "apply",
                "self_apply",
                json!({"reason":"Register source and configuration"}),
            ),
            "tool_calls",
        );
    }
    for (prompt, tool, arguments) in [
        (
            RENAME,
            "plugin_rename",
            json!({"name":"alpha", "to":"beta", "reason":"Give the plugin its new name"}),
        ),
        (HISTORY, "self_history", json!({"to":"home"})),
    ] {
        if messages.iter().any(|message| message["content"] == prompt) {
            return response(tool_message(tool, tool, arguments), "tool_calls");
        }
    }
    let call = |id: &str, name: &str| tool_message(id, name, json!({}))["tool_calls"][0].clone();
    let mut calls: Vec<_> = (0..enco_kernel::TRIAL_CALLS)
        .map(|index| call(&format!("echo-{index}"), "alpha_echo"))
        .collect();
    calls.push(call("status", "self_status"));
    let message = json!({"role": "assistant", "content": null, "tool_calls": calls});
    response(message, "tool_calls")
}

async fn tool_results(daemon: &Daemon, text: &str) -> Vec<Entry> {
    let session = format!("tools-{}", SessionId::new());
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: session.clone(),
        })
        .await
        .unwrap();
    client
        .request(Command::Send {
            images: Vec::new(),
            session: session.clone(),
            text: text.into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    finish(&mut client).await
}

async fn use_source(daemon: &Daemon) -> Value {
    let entries = tool_results(daemon, "exercise the plugin and inspect the node").await;
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(
                entry.body,
                EntryBody::ToolCallSettled {
                    outcome: Settlement::Ok,
                    ..
                }
            ))
            .count(),
        enco_kernel::TRIAL_CALLS as usize + 1
    );
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        running_generation(&status["plugins"]["alpha"])["status"],
        json!(GenerationStatus::Healthy)
    );
    status
}

const EMPTY_CHECK: &str = r#"if config != "{}""#;
const LABEL_CHECK: &str = r##"if config != "{}" && config != r#"{"label":"value"}"#"##;
const EMPTY_SCHEMA: &str = r#"{"type":"object","additionalProperties":false}"#;
const LABEL_SCHEMA: &str =
    r#"{"type":"object","properties":{"label":{"type":"string"}},"additionalProperties":false}"#;
/// Owner text around the generated region, which registration must preserve.
const OWNER_README: &str = concat!(
    "Owner introduction\n",
    "<!-- enco:generated:begin -->\n<!-- enco:generated:end -->\n",
    "Owner examples\n"
);

fn write_owner_files(home: &Path) -> Vec<u8> {
    let config_path = home.join("config.toml");
    let config_bytes = format!(
        "# Owner configuration notes\n{}",
        std::fs::read_to_string(&config_path).unwrap()
    )
    .into_bytes();
    std::fs::write(&config_path, &config_bytes).unwrap();
    std::fs::create_dir_all(home.join("plugins/alpha/target")).unwrap();
    std::fs::write(home.join("plugins/alpha/target/owner-cache"), b"keep").unwrap();
    std::fs::write(home.join("plugins/alpha/.git"), b"owner metadata").unwrap();
    config_bytes
}

#[tokio::test]
async fn joint_source_and_configuration_restore_reuses_the_retained_artifact() {
    let service = echo_service().await;
    let mut daemon = Daemon::start(&service, "openai-compatible").await;
    let home = daemon.root.path().to_path_buf();
    support::build_target(&home, "alpha");
    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "alpha".parse().unwrap(),
        })
        .await
        .unwrap();
    let source = home.join("plugins/alpha/src/lib.rs");
    let original = std::fs::read_to_string(&source).unwrap();
    let code = original
        .replace(EMPTY_CHECK, LABEL_CHECK)
        .replace(EMPTY_SCHEMA, LABEL_SCHEMA);
    std::fs::write(&source, &code).unwrap();
    let readme = home.join("plugins/alpha/README.md");
    std::fs::write(&readme, OWNER_README).unwrap();
    let mut document: Value =
        toml::from_str(&Daemon::config(&service, "openai-compatible")).unwrap();
    document["plugin"] = json!({"alpha": {"config": {"label": "value"}}});
    write_config(&daemon, &document);
    let config_bytes = write_owner_files(&home);
    let first = acceptance::register_with_agent(&daemon).await;
    let first_code = &first["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["kind"] == "code" && change["name"] == "alpha")
        .unwrap()["generation"];
    use_source(&daemon).await;
    let registration = serde_json::from_value(first["id"].clone()).unwrap();
    check_readme(&readme);
    check_build_rejection(&daemon, &document, &source).await;
    std::fs::write(&source, code.replace("label", "caption")).unwrap();
    document["plugin"]["alpha"]["config"] = json!({"caption": "value"});
    write_config(&daemon, &document);
    let second = apply(&daemon).await;
    assert_eq!(
        second["registered"]["generations"][0]["registration"],
        second["registered"]["configuration"]["registration"]
    );
    assert_ne!(
        second["registered"]["generations"][0]["artifact"],
        first_code["artifact"]
    );
    daemon
        .connect()
        .await
        .request(Command::Rewind {
            output: protocol::Output::Json,
            arguments: enco_host::development::RewindArguments {
                registration,
                paths: None,
            },
        })
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(home.join("config.toml")).unwrap(),
        config_bytes
    );
    assert_eq!(
        std::fs::read(home.join("plugins/alpha/target/owner-cache")).unwrap(),
        b"keep"
    );
    assert_eq!(
        std::fs::read(home.join("plugins/alpha/.git")).unwrap(),
        b"owner metadata"
    );
    daemon.stop().await;
    daemon.restart_without(&["PATH"]).await;
    let restored = apply(&daemon).await;
    assert_eq!(restored["builds"][0]["reused"], true);
    let restored_code = &restored["registered"]["generations"][0];
    assert_eq!(restored_code["artifact"], first_code["artifact"]);
    assert_eq!(restored_code["plugin"], first_code["plugin"]);
    assert_eq!(
        restored_code["settings"]["config"],
        json!({"label":"value"})
    );
    let status = use_source(&daemon).await;
    assert_eq!(status["configuration"]["home"]["state"], "same");
    assert_eq!(status["plugins"]["alpha"]["home"]["code"]["state"], "same");
    assert_eq!(
        status["plugins"]["alpha"]["running"]["standing"],
        json!({"standing": "registered", "differences": []})
    );
    check_rollback_does_not_replay_intent(&daemon).await;
    check_rename(&mut daemon, &mut document, &first_code["plugin"]).await;
    daemon.stop().await;
}

fn check_readme(readme: &Path) {
    let generated = std::fs::read_to_string(readme).unwrap();
    assert!(
        generated.starts_with("Owner introduction\n") && generated.ends_with("\nOwner examples\n")
    );
    assert!(
        generated.contains("alpha_echo") && generated.contains("Return the given text unchanged.")
    );
}

async fn check_rollback_does_not_replay_intent(daemon: &Daemon) {
    let mut client = daemon.connect().await;
    client.request(Command::Rollback {}).await.unwrap();
    let before = client
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert!(apply(daemon).await["registered"].is_null());
    let after = client
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        after["configuration"]["running"],
        before["configuration"]["running"]
    );
    assert_eq!(after["plugins"], before["plugins"]);
}

async fn check_rename(daemon: &mut Daemon, document: &mut Value, identity: &Value) {
    let rename = || Command::PluginRename {
        output: protocol::Output::Json,
        name: "alpha".parse().unwrap(),
        to: "beta".parse().unwrap(),
        reason: None,
    };
    let error = daemon
        .connect()
        .await
        .request(rename())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("plugin.alpha"), "{error}");
    document["plugin"] = json!({});
    write_config(daemon, document);
    let settings = apply(daemon).await;
    let source = daemon.root.path().join("plugins/alpha/src/lib.rs");
    let accepted = std::fs::read_to_string(&source).unwrap();
    std::fs::write(
        &source,
        format!("{accepted}\n// Unregistered owner notes.\n"),
    )
    .unwrap();
    let renamed = tool_results(daemon, RENAME).await;
    assert!(renamed.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            ..
        }
    )));
    let history = tool_results(daemon, HISTORY).await;
    assert!(history.iter().any(|entry| matches!(&entry.body, EntryBody::ToolCallSettled { outcome: Settlement::Ok, content, .. } if content.contains("plugins/beta/src/lib.rs") && content.contains("Unregistered owner notes"))));
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        status["recent_registrations"][0]["initiator"]["kind"],
        "agent"
    );
    assert_eq!(
        status["recent_registrations"][0]["initiator"]["reason"],
        "Give the plugin its new name"
    );
    assert!(
        status["recent_registrations"][0]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["kind"] == "renamed"
                && change["name"] == "beta"
                && change["from"] == "alpha")
    );
    assert_eq!(&status["plugins"]["beta"]["running"]["id"], identity);
    assert_eq!(
        running_generation(&status["plugins"]["beta"])["id"],
        settings["registered"]["generations"][0]["id"]
    );
    assert_eq!(
        status["plugins"]["beta"]["home"]["code"]["state"],
        "unbuilt"
    );
    assert!(
        std::fs::read_to_string(daemon.root.path().join("plugins/beta/src/lib.rs"))
            .unwrap()
            .contains("Unregistered owner notes")
    );
    check_home_moves_do_not_rename_registered_plugin(daemon).await;
}

async fn check_home_moves_do_not_rename_registered_plugin(daemon: &mut Daemon) {
    std::fs::rename(
        daemon.root.path().join("plugins/beta"),
        daemon.root.path().join("plugins/alpha"),
    )
    .unwrap();
    daemon.stop().await;
    daemon.restart().await;
    let restarted = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        restarted["plugins"]["beta"]["running"]["running"]["name"],
        "beta"
    );
    std::fs::rename(
        daemon.root.path().join("plugins/alpha"),
        daemon.root.path().join("plugins/beta"),
    )
    .unwrap();
    std::fs::rename(
        daemon.root.path().join("plugins/beta"),
        daemon.root.path().join("plugins/gamma"),
    )
    .unwrap();
    let error = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("plugin_rename"));
}

#[tokio::test]
async fn first_start_registers_a_source_provider_with_the_configuration_that_uses_it() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".data")).unwrap();
    support::build_target(root.path(), "provider");
    support::copy_plugin("openai-compatible", root.path());
    std::fs::rename(
        root.path().join("plugins/openai-compatible"),
        root.path().join("plugins/local"),
    )
    .unwrap();
    let mut document: Value = toml::from_str(&Daemon::config(&server, "local")).unwrap();
    document["endpoint"]["embed"]["plugin"] = "local".into();
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let mut daemon = Daemon::start_home(root).await;
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    let generation = running_generation(&status["plugins"]["local"]);
    assert_eq!(generation["origin"], "code");
    assert_eq!(
        generation["registration"],
        status["configuration"]["running"]["registration"]
    );
    assert!(
        status["configuration"]["unavailable_uses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
    let home = daemon.root.path();
    for name in ["enco.db", "enco.db-wal", "enco.db-shm"] {
        let file = home.join(".data").join(name);
        if file.exists() {
            std::fs::remove_file(file).unwrap();
        }
    }
    daemon.restart().await;
    let rebuilt = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(rebuilt["recent_registrations"].as_array().unwrap().len(), 1);
    assert_eq!(
        rebuilt["configuration"]["running"]["registration"],
        rebuilt["recent_registrations"][0]["id"]
    );
    check_references_keep_the_provider(&daemon, &rebuilt).await;
    assert!(server.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
}

async fn check_references_keep_the_provider(daemon: &Daemon, registered: &Value) {
    let error = daemon
        .connect()
        .await
        .request(Command::PluginRename {
            output: protocol::Output::Json,
            name: "local".parse().unwrap(),
            to: "renamed".parse().unwrap(),
            reason: None,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("profile default.reply") && error.contains("memory.embedding"),
        "{error}"
    );
    std::fs::remove_dir_all(daemon.root.path().join("plugins/local")).unwrap();
    let error = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile default.reply"), "{error}");
    let rejected = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        rejected["recent_registrations"],
        registered["recent_registrations"]
    );
    assert_eq!(
        rejected["plugins"]["local"]["running"],
        registered["plugins"]["local"]["running"]
    );
}

async fn check_build_rejection(daemon: &Daemon, document: &Value, source: &Path) {
    let home = daemon.root.path();
    let before = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "broken".parse().unwrap(),
        })
        .await
        .unwrap();
    std::fs::write(
        home.join("plugins/broken/src/lib.rs"),
        "fn broken() { ??? }",
    )
    .unwrap();
    let mut changed = document.clone();
    changed["owner"]["timezone"] = "Europe/Paris".into();
    write_config(daemon, &changed);
    let error = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("broken") && error.contains("src/lib.rs:"),
        "{error}"
    );
    let full = error
        .lines()
        .find_map(|line| line.strip_prefix("Full log: "))
        .unwrap()
        .trim_matches('`');
    assert!(std::fs::read_to_string(full).unwrap().contains("error"));
    let after = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        after["recent_registrations"],
        before["recent_registrations"]
    );
    assert_eq!(
        after["configuration"]["running"],
        before["configuration"]["running"]
    );
    std::fs::remove_dir_all(home.join("plugins/broken")).unwrap();
    write_config(daemon, document);
    let code = std::fs::read_to_string(source).unwrap();
    std::fs::write(
        source,
        format!("{code}\n// Owner explanation; behavior is unchanged.\n"),
    )
    .unwrap();
    let unchanged = apply(daemon).await;
    assert!(unchanged["registered"].is_null());
    // The comment changed the source tree, so Cargo built it before comparing artifacts.
    assert_eq!(unchanged["builds"][0]["name"], "alpha");
    assert_eq!(unchanged["builds"][0]["reused"], false);
}
