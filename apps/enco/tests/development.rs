#![expect(clippy::unwrap_used, reason = "test failures retain their context")]
#[path = "development/acceptance.rs"]
mod acceptance;
#[path = "../src/client.rs"]
mod client;
#[path = "../src/protocol.rs"]
mod protocol;
#[path = "development/sdk.rs"]
mod sdk;
mod support;

use enco_core::{
    CallId, EntryBody, EventId, GenerationStatus, Outcome, PluginSettings, SessionId, Settlement,
    code,
};
use enco_host::{SqliteStore, StorePaths};
use enco_kernel::{CallContext, Runtime, Store};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use support::{Daemon, embeddings, finish, git, response, tool_message};
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
        .request(Command::Status {})
        .await
        .unwrap();
    let artifact = status["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "fixture")
        .unwrap()["active"]["artifact"]
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
        .request(Command::Apply {})
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
        return response(tool_message("apply", "self_apply", json!({})), "tool_calls");
    }
    let call = |id: &str, name: &str| tool_message(id, name, json!({}))["tool_calls"][0].clone();
    let mut calls: Vec<_> = (0..enco_kernel::TRIAL_CALLS)
        .map(|index| call(&format!("echo-{index}"), "alpha_echo"))
        .collect();
    calls.push(call("status", "self_status"));
    let message = json!({"role": "assistant", "content": null, "tool_calls": calls});
    response(message, "tool_calls")
}

async fn tool_results(daemon: &Daemon, text: &str) -> Vec<Value> {
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
            session: session.clone(),
            text: text.into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let log = finish(&mut client).await;
    let root = daemon.root.path();
    let store = SqliteStore::open(StorePaths {
        db: root.join(".data/enco.db"),
        blobs: root.join(".data/blobs"),
        artifacts: root.join(".data/artifacts"),
    })
    .await
    .unwrap();
    let mut results = Vec::new();
    for entry in log {
        if let EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            content,
            full,
            ..
        } = entry.body
        {
            let content = match full {
                Some(hash) => String::from_utf8(store.get_blob(&hash).await.unwrap()).unwrap(),
                None => content,
            };
            let value: Value = serde_json::from_str(&content).unwrap();
            results.push(value);
        }
    }
    results
}

async fn use_source(daemon: &Daemon) -> Value {
    let status = tool_results(daemon, "exercise the plugin and inspect the node")
        .await
        .into_iter()
        .find(|value| value.get("host").is_some())
        .unwrap();
    let cli = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    for key in ["host", "config", "registered", "registrations", "workspace"] {
        assert_eq!(status[key], cli[key]);
    }
    let plugin = cli["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "alpha")
        .unwrap();
    assert_eq!(plugin["active"]["status"], json!(GenerationStatus::Healthy));
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
            name: "alpha".into(),
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
    let first = acceptance::apply_preserving_owner_history(&daemon).await;
    let first_code = &first["registered"]["generations"][0];
    let observed = use_source(&daemon).await;
    let revision = observed["registered"]["base"]["revision"].as_str().unwrap();
    let generated = std::fs::read_to_string(readme).unwrap();
    assert!(
        generated.starts_with("Owner introduction\n") && generated.ends_with("\nOwner examples\n")
    );
    assert!(
        generated.contains("alpha_echo")
            && generated.contains("Return the supplied object unchanged.")
    );
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
    git(
        &home,
        &[
            "restore",
            "--source",
            revision,
            "--",
            "plugins/alpha",
            "config.toml",
        ],
    )
    .await;
    daemon.stop().await;
    daemon.restart_without(&["PATH"]).await;
    let restored = apply(&daemon).await;
    assert_eq!(restored["builds"][0]["reused"], true);
    assert_eq!(
        restored["registered"]["generations"][0]["artifact"],
        first_code["artifact"]
    );
    assert_eq!(
        restored["registered"]["generations"][0]["plugin"],
        first_code["plugin"]
    );
    assert_eq!(
        restored["registered"]["generations"][0]["settings"]["config"],
        json!({"label":"value"})
    );
    let status = use_source(&daemon).await;
    assert_eq!(status["workspace"]["configuration"]["state"], "unchanged");
    assert_eq!(
        status["workspace"]["plugins"]["alpha"]["code"]["state"],
        "unchanged"
    );
    assert!(
        status["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin["name"] == "alpha")
            .unwrap()["matches_registered"]
            .as_bool()
            .unwrap()
    );
    check_rollback_does_not_replay_intent(&daemon).await;
    check_rename(&mut daemon, &mut document, &first_code["plugin"]).await;
    daemon.stop().await;
}

async fn check_rollback_does_not_replay_intent(daemon: &Daemon) {
    let mut client = daemon.connect().await;
    client.request(Command::Rollback {}).await.unwrap();
    let before = client.request(Command::Status {}).await.unwrap();
    assert!(apply(daemon).await["registered"].is_null());
    let after = client.request(Command::Status {}).await.unwrap();
    assert_eq!(after["config"]["active"], before["config"]["active"]);
    assert_eq!(after["plugins"], before["plugins"]);
}

async fn check_rename(daemon: &mut Daemon, document: &mut Value, identity: &Value) {
    let rename = || Command::PluginRename {
        name: "alpha".into(),
        to: "beta".into(),
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
    daemon.connect().await.request(rename()).await.unwrap();
    let status = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    let plugin = status["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "beta")
        .unwrap();
    assert_eq!(&plugin["id"], identity);
    assert_eq!(
        plugin["active"]["id"],
        settings["registered"]["generations"][0]["id"]
    );
    assert_eq!(
        status["workspace"]["plugins"]["beta"]["code"]["state"],
        "unbuilt"
    );
    assert_eq!(
        git(
            daemon.root.path(),
            &["show", "enco/registered:plugins/beta/src/lib.rs"]
        )
        .await,
        accepted.trim()
    );
    assert!(
        std::fs::read_to_string(daemon.root.path().join("plugins/beta/src/lib.rs"))
            .unwrap()
            .contains("Unregistered owner notes")
    );
    git(daemon.root.path(), &["add", "plugins/beta"]).await;
    git(daemon.root.path(), &["mv", "plugins/beta", "plugins/alpha"]).await;
    daemon.stop().await;
    daemon.restart().await;
    let restarted = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    assert_eq!(
        restarted["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| &plugin["id"] == identity)
            .unwrap()["name"],
        "beta"
    );
    git(daemon.root.path(), &["mv", "plugins/alpha", "plugins/beta"]).await;
    git(daemon.root.path(), &["add", "plugins/beta"]).await;
    git(daemon.root.path(), &["mv", "plugins/beta", "plugins/gamma"]).await;
    let error = daemon
        .connect()
        .await
        .request(Command::Apply {})
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
        .request(Command::Status {})
        .await
        .unwrap();
    let plugin = status["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "local")
        .unwrap();
    assert_eq!(plugin["active"]["origin"], "code");
    assert_eq!(
        plugin["active"]["registration"],
        status["config"]["active"]["registration"]
    );
    assert!(
        status["config"]["unavailable_uses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
    let home = daemon.root.path();
    let accepted = git(home, &["rev-parse", "enco/registered"]).await;
    for name in ["enco.db", "enco.db-wal", "enco.db-shm"] {
        let file = home.join(".data").join(name);
        if file.exists() {
            std::fs::remove_file(file).unwrap();
        }
    }
    daemon.restart().await;
    assert_eq!(
        git(daemon.root.path(), &["rev-parse", "enco/registered^"]).await,
        accepted
    );
    let rebuilt = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    assert_eq!(rebuilt["registrations"].as_array().unwrap().len(), 1);
    assert_eq!(
        rebuilt["config"]["active"]["document"],
        status["config"]["active"]["document"]
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
            name: "local".into(),
            to: "renamed".into(),
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
        .request(Command::Apply {})
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile default.reply"), "{error}");
    let rejected = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    assert_eq!(rejected["registrations"], registered["registrations"]);
    assert_eq!(rejected["plugins"], registered["plugins"]);
}

async fn check_build_rejection(daemon: &Daemon, document: &Value, source: &Path) {
    let home = daemon.root.path();
    let before = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    let revision = git(home, &["rev-parse", "enco/registered"]).await;
    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "broken".into(),
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
        .request(Command::Apply {})
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
        .unwrap();
    assert!(std::fs::read_to_string(full).unwrap().contains("error"));
    assert_eq!(git(home, &["rev-parse", "enco/registered"]).await, revision);
    let after = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    assert_eq!(after["registrations"], before["registrations"]);
    assert_eq!(after["config"]["active"], before["config"]["active"]);
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
    assert_eq!(git(home, &["rev-parse", "enco/registered"]).await, revision);
}
