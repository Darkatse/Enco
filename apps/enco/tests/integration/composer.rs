#![expect(clippy::unwrap_used, reason = "fixtures fail the test with context")]
use crate::{
    protocol::{Command, Output},
    support::{self, *},
};
use enco_core::{
    Entry, EntryBody, EventId, GenerationId, GenerationStatus, RoundEnd, RunEnd, Settlement, code,
};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

const SOURCE: &str = r##"//! A composer fixture exercising registered policy and its restricted runtime.
use bindings::exports::enco::plugin::{composer, lifecycle};
use enco_sdk::{host, types::{Failure, Role}};
use serde::Deserialize;

enco_sdk::export!(composer, Plugin);
struct Plugin;
const DISCLOSE: bool = true;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// A marker that identifies the profile options or plugin configuration.
    #[serde(default)]
    label: String,
}

fn failure(message: impl Into<String>) -> Failure {
    enco_sdk::protocol::failure("plugin.contract", message, false)
}

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, Failure> {
        let settings: Settings = serde_json::from_str(&config).map_err(|error| failure(error.to_string()))?;
        Ok(lifecycle::Description {
            summary: format!("Test composer {}", settings.label),
            config_schema: enco_sdk::input_schema::<Settings>().to_string(),
            credentials: vec![],
        })
    }

    async fn probe() -> Result<(), Failure> {
        if host::config() != "{}" {
            return Err(failure("probe received the registered configuration"));
        }
        let result = host::http(host::HttpRequest {
            method: "GET".into(), url: "__PROBE_URL__".into(),
            headers: vec![], body: None, timeout_ms: Some(1000),
        }).await;
        match result {
            Err(error) if matches!(error.kind, host::HttpErrorKind::InvalidRequest) && !error.request_sent => Ok(()),
            _ => Err(failure("probe reached the network")),
        }
    }
}

impl composer::Guest for Plugin {
    fn options_schema(_config: String) -> String {
        enco_sdk::input_schema::<Settings>().to_string()
    }

    fn compose(options: String, input: composer::ComposeInput) -> Result<composer::Composition, Failure> {
        if std::fs::read(std::path::Path::new(&input.home).join("config.toml")).is_ok() {
            return Err(failure("composer read a host file"));
        }
        if std::env::var("ENCO_TEST_API_KEY").is_ok() {
            return Err(failure("composer read the host environment"));
        }
        let options = enco_sdk::parse_options::<Settings>(&options)?;
        let config: Settings = serde_json::from_str(&host::config()).map_err(|error| failure(error.to_string()))?;
        let mut items = vec![composer::PlanItem::Message(composer::Inline {
            role: Role::System,
            text: format!("custom-composer {} {}", options.label, config.label),
            sources: vec![],
        })];
        items.extend(input.transcript.items.into_iter().map(|item| composer::PlanItem::Log(composer::LogRef {
            pos: item.pos, images: false,
        })));
        Ok(composer::Composition::Reply(composer::Plan {
            items,
            tools: if DISCLOSE { input.capabilities.into_iter().map(|capability| capability.id).collect() } else { vec![] },
            max_output_tokens: Some(input.reply.budget.max_output_tokens),
            omitted: vec![],
        }))
    }
}
"##;

async fn status(daemon: &Daemon) -> Value {
    daemon
        .connect()
        .await
        .request(Command::Status {
            output: Output::Json,
        })
        .await
        .unwrap()
}

async fn apply(daemon: &Daemon) {
    daemon
        .connect()
        .await
        .request(Command::Apply {
            output: Output::Json,
            reason: None,
        })
        .await
        .unwrap();
}

async fn send(daemon: &Daemon, session: &str, text: &str) -> Vec<Entry> {
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: session.into(),
        })
        .await
        .unwrap();
    client
        .request(Command::Send {
            session: session.into(),
            text: text.into(),
            images: vec![],
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    finish(&mut client).await
}

fn service(request: &wiremock::Request) -> ResponseTemplate {
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    let messages = body["messages"].as_array().unwrap();
    let custom = messages
        .iter()
        .any(|message| message["content"] == "custom-composer profile-label configured-label");
    let recovering = messages
        .iter()
        .any(|message| message["content"] == "recover through factory");
    if recovering && !custom && !messages.iter().any(|message| message["role"] == "tool") {
        response(
            tool_message(
                "repair",
                "fs_write",
                json!({"path":"recovered.txt", "content":"factory recovery"}),
            ),
            "tool_calls",
        )
    } else {
        response(json!({"role":"assistant", "content":"done"}), "stop")
    }
}

#[tokio::test]
async fn source_composer_runs_with_profile_options_and_factory_recovers_a_bad_trial() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/probe"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path("/chat/completions"))
        .respond_with(service)
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let home = root.path().to_path_buf();
    std::fs::create_dir(home.join(".data")).unwrap();
    std::fs::write(
        home.join("config.toml"),
        Daemon::config(&server, "openai-compatible"),
    )
    .unwrap();
    support::build_target(&home, "composer-policy");
    support::initialize(&home).await;
    let mut daemon = Daemon::start_home(root).await;
    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "custom".parse().unwrap(),
        })
        .await
        .unwrap();
    let source = SOURCE.replace("__PROBE_URL__", &format!("{}/probe", server.uri()));
    let file = home.join("plugins/custom/src/lib.rs");
    std::fs::write(&file, &source).unwrap();
    let mut document: Value =
        toml::from_str(&Daemon::config(&server, "openai-compatible")).unwrap();
    document["plugin"]["custom"]["config"] = json!({"label":"configured-label"});
    document["profile"]["default"]["composer"] =
        json!({"plugin":"custom", "options":{"label":"profile-label"}});
    std::fs::write(
        home.join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    apply(&daemon).await;
    let registered = status(&daemon).await;
    assert!(registered["plugins"]["custom"]["registered"]["factory"].is_null());
    let good: GenerationId =
        serde_json::from_value(running_generation(&registered["plugins"]["custom"])["id"].clone())
            .unwrap();
    let factory: GenerationId = serde_json::from_value(
        registered["plugins"]["composer"]["registered"]["factory"]["generation"].clone(),
    )
    .unwrap();
    assert_profile_composer(&daemon, &server, good).await;
    std::fs::write(
        &file,
        source.replace(
            "const DISCLOSE: bool = true;",
            "const DISCLOSE: bool = false;",
        ),
    )
    .unwrap();
    apply(&daemon).await;
    let trial = status(&daemon).await;
    let bad = running_generation(&trial["plugins"]["custom"])["id"].clone();
    assert_ne!(bad, json!(good));
    assert_factory_recovery(&daemon, factory, good, bad).await;
    daemon.stop().await;
}

async fn assert_profile_composer(daemon: &Daemon, server: &MockServer, good: GenerationId) {
    for index in 0..enco_kernel::TRIAL_CALLS {
        let log = send(
            daemon,
            &format!("custom-{index}"),
            "use the profile composer",
        )
        .await;
        assert!(matches!(
            log.last().unwrap().body,
            EntryBody::RunEnded {
                end: RunEnd::Completed,
                ..
            }
        ));
        assert!(log.iter().any(|entry| matches!(entry.body,
            EntryBody::AttemptStarted { composer, .. } if composer == good)));
    }
    assert_eq!(
        running_generation(&status(daemon).await["plugins"]["custom"])["status"],
        json!(GenerationStatus::Healthy)
    );
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().any(|request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["content"] == "custom-composer profile-label configured-label"
            })
        })
    }));
}

async fn assert_factory_recovery(
    daemon: &Daemon,
    factory: GenerationId,
    good: GenerationId,
    bad: Value,
) {
    let log = send(daemon, "recovery", "recover through factory").await;
    assert!(log.iter().any(|entry| matches!(&entry.body,
        EntryBody::RoundEnded { end: RoundEnd::Failed { failure, .. }, .. } if failure.code == code::PLUGIN_CONTRACT)));
    assert!(log.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RoundStarted {
            safe_mode: true,
            ..
        }
    )));
    let composers: Vec<_> = log
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted { composer, .. } => Some(composer),
            _ => None,
        })
        .collect();
    assert!(!composers.is_empty());
    assert!(composers.iter().all(|id| *id == factory));
    assert!(log.iter().any(|entry| matches!(
        entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            ..
        }
    )));
    assert_eq!(
        std::fs::read_to_string(daemon.root.path().join("workspace/recovered.txt")).unwrap(),
        "factory recovery"
    );
    let attempt = log
        .iter()
        .rev()
        .find_map(|entry| match entry.body {
            EntryBody::AttemptStarted { attempt, .. } => Some(attempt),
            _ => None,
        })
        .unwrap();
    let inspected: enco_kernel::Inspection = serde_json::from_value(
        daemon
            .connect()
            .await
            .request(Command::Inspect {
                session: "recovery".into(),
                attempt_id: Some(attempt),
            })
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(enco_host::LIFELINE.iter().all(|name| {
        inspected
            .plan
            .tools
            .iter()
            .any(|(_, tool)| tool.name == *name)
    }));
    let restored = status(daemon).await;
    assert_eq!(
        running_generation(&restored["plugins"]["custom"])["id"],
        json!(good)
    );
    assert_eq!(
        restored["plugins"]["custom"]["registered"]["activation"]["generation"]["id"],
        bad
    );
    assert_eq!(
        restored["plugins"]["custom"]["registered"]["activation"]["generation"]["status"],
        json!(GenerationStatus::Failed)
    );
}
