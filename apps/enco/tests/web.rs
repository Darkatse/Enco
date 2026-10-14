#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
#[path = "../src/client.rs"]
mod client;
#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::*;
use enco_kernel::{Inspection, Runtime};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::{Value, json};
use support::*;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

struct Services {
    model: MockServer,
    jina: MockServer,
    exa: MockServer,
    tinyfish: MockServer,
}

impl Services {
    async fn new() -> Self {
        let model = MockServer::start().await;
        embeddings(&model).await;
        Self {
            model,
            jina: MockServer::start().await,
            exa: MockServer::start().await,
            tinyfish: MockServer::start().await,
        }
    }

    fn document(&self, engines: &[&str]) -> Value {
        let mut document: Value =
            toml::from_str(&Daemon::config(&self.model, "openai-compatible")).unwrap();
        document["endpoint"]["chat"]
            .as_object_mut()
            .unwrap()
            .remove("api_key_env");
        let mut secrets = json!({});
        if engines.contains(&"exa") {
            secrets["exa"] = "ENCO_TEST_API_KEY".into();
        }
        if engines.contains(&"tinyfish") {
            secrets["tinyfish"] = "ENCO_TEST_SECOND_API_KEY".into();
        }
        document["plugin"] = json!({"web": {"config": {"search": engines, "urls": {"jina": self.jina.uri(), "exa": self.exa.uri(), "tinyfish": self.tinyfish.uri()}}, "secrets": secrets}});
        document
    }

    async fn pages(&self) {
        Mock::given(path("/https://news.test/story"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"data": {"httpStatus": 403, "content": "Access denied"}}),
                ),
            )
            .mount(&self.jina)
            .await;
        Mock::given(path("/contents")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [{"url":"https://news.test/story", "text":"The page says 42. enco-test-secret enco-second-test-secret"}], "statuses": [{"status":"success"}]}))).mount(&self.exa).await;
        Mock::given(path("/search")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [{"title":"News", "url":"https://news.test/story", "publishedDate":"2026-10-01", "highlights":["A source-backed answer"]}]}))).mount(&self.exa).await;
        Mock::given(path("/")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [{"title":"News", "url":"https://news.test/story", "snippet":"Another source-backed answer"}]}))).mount(&self.tinyfish).await;
    }
}

async fn send(daemon: &Daemon) -> Vec<Entry> {
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    client
        .request(Command::Send {
            session: "main".into(),
            event_id: EventId::new(),
            text: "read the news".into(),
        })
        .await
        .unwrap();
    finish(&mut client).await
}

async fn web_status(daemon: &Daemon) -> Value {
    let status = daemon
        .connect()
        .await
        .request(Command::PluginStatus {})
        .await
        .unwrap();
    status
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "web")
        .unwrap()
        .clone()
}

async fn config_generation_count(daemon: &Daemon) -> usize {
    daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap()["config"]["generations"]
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test]
async fn wasm_tools_use_origin_bound_credentials_and_report_fetch_fallback() {
    let services = Services::new().await;
    services.pages().await;
    Mock::given(path("/chat/completions")).respond_with(|request: &wiremock::Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        if body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "tool") {
            return response(json!({"role":"assistant", "content":"done"}), "stop");
        }
        let calls = [
            tool_message("fetch", "web_fetch", json!({"url":"https://news.test/story"})),
            tool_message("exa", "web_search", json!({"query":"recent news", "engine":"exa", "limit":3, "since":"2026-10-01"})),
            tool_message("tinyfish", "web_search", json!({"query":"recent news", "engine":"tinyfish", "limit":2, "since":"2026-10-01"})),
        ].into_iter().map(|message| message["tool_calls"][0].clone()).collect::<Vec<_>>();
        response(json!({"role":"assistant", "content":null, "tool_calls":calls}), "tool_calls")
    }).mount(&services.model).await;
    let mut daemon =
        Daemon::start_config(&toml::to_string(&services.document(&["exa", "tinyfish"])).unwrap())
            .await;
    let generation: GenerationId =
        serde_json::from_value(web_status(&daemon).await["active"]["id"].clone()).unwrap();
    let log = send(&daemon).await;
    assert_eq!(log.iter().filter(|entry| matches!(&entry.body, EntryBody::ToolCallStarted { code: CodeRef::Generation { id }, .. } if *id == generation)).count(), 3);
    let results: Vec<_> = log
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                content,
                outcome: Settlement::Ok,
                ..
            } => Some(content),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3);
    assert!(
        results[0].starts_with("Source: Exa")
            && results[0].contains("Jina Reader failed: page returned HTTP 403")
    );
    assert!(
        results[0].contains("[redacted:ENCO_TEST_API_KEY]")
            && results[0].contains("[redacted:ENCO_TEST_SECOND_API_KEY]")
    );
    let tinyfish: Value = serde_json::from_str(results[2]).unwrap();
    assert!(tinyfish["results"][0]["published"].is_null());
    let serialized = serde_json::to_string(&log).unwrap();
    assert!(
        !serialized.contains("enco-test-secret") && !serialized.contains("enco-second-test-secret")
    );
    for request in services.exa.received_requests().await.unwrap() {
        assert_eq!(request.headers["x-api-key"], "enco-test-secret");
    }
    for request in services.jina.received_requests().await.unwrap() {
        assert!(
            !request.headers.contains_key("x-api-key")
                && !request.headers.contains_key("authorization")
        );
    }
    let tinyfish = services.tinyfish.received_requests().await.unwrap();
    assert_eq!(tinyfish[0].headers["x-api-key"], "enco-second-test-secret");
    assert!(
        tinyfish[0]
            .url
            .query_pairs()
            .any(|(name, value)| name == "after_date" && value == "2026-10-01")
    );
    daemon.stop().await;
}

#[tokio::test]
async fn missing_credentials_reject_apply_but_only_exclude_the_plugin_on_restart() {
    let services = Services::new().await;
    Mock::given(path("/chat/completions"))
        .respond_with(response(
            json!({"role":"assistant", "content":"done"}),
            "stop",
        ))
        .mount(&services.model)
        .await;
    let mut document = services.document(&[]);
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let before = web_status(&daemon).await;
    let config_before = config_generation_count(&daemon).await;
    document = services.document(&["exa"]);
    document["plugin"]["web"]["secrets"]["exa"] = UNSET_API_KEY_ENV.into();
    std::fs::write(
        daemon.root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let rejected = daemon
        .connect()
        .await
        .request(Command::ConfigApply { generation: None })
        .await;
    assert!(
        rejected
            .unwrap_err()
            .to_string()
            .contains(UNSET_API_KEY_ENV)
    );
    assert_eq!(
        web_status(&daemon).await["generations"],
        before["generations"]
    );
    document["plugin"]["web"]["secrets"]["exa"] = "ENCO_TEST_API_KEY".into();
    std::fs::write(
        daemon.root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    daemon
        .connect()
        .await
        .request(Command::ConfigApply { generation: None })
        .await
        .unwrap();
    send(&daemon).await;
    let applied = web_status(&daemon).await;
    assert_eq!(
        applied["generations"].as_array().unwrap().len(),
        before["generations"].as_array().unwrap().len() + 1
    );
    let config_after = config_generation_count(&daemon).await;
    assert_eq!(config_after, config_before + 1);
    let active = applied["active"].clone();
    daemon.stop().await;
    daemon.restart_without(&["ENCO_TEST_API_KEY"]).await;
    let unavailable = web_status(&daemon).await;
    assert_eq!(unavailable["active"], active);
    assert!(
        unavailable["unavailable"]
            .to_string()
            .contains("ENCO_TEST_API_KEY")
    );
    send(&daemon).await;
    let requests = services.model.received_requests().await.unwrap();
    let chats: Vec<Value> = requests
        .iter()
        .filter(|request| request.url.path() == "/chat/completions")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    let search = chats[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "web_search")
        .unwrap();
    assert!(
        search["function"]["parameters"]["properties"]
            .get("engine")
            .is_none()
    );
    assert!(chats[1]["tools"].as_array().unwrap().iter().all(|tool| {
        !tool["function"]["name"]
            .as_str()
            .unwrap()
            .starts_with("web_")
    }));
    assert!(
        chats[1]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("ENCO_TEST_API_KEY")
    );
    daemon.stop().await;
}

#[tokio::test]
async fn service_failures_settle_without_failing_the_tool_generation() {
    let services = Services::new().await;
    Mock::given(path("/https://news.test/story"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&services.jina)
        .await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                response(json!({"role":"assistant", "content":"unavailable"}), "stop")
            } else {
                response(
                    tool_message(
                        "fetch",
                        "web_fetch",
                        json!({"url":"https://news.test/story"}),
                    ),
                    "tool_calls",
                )
            }
        })
        .mount(&services.model)
        .await;
    let mut daemon = Daemon::start_config(&toml::to_string(&services.document(&[])).unwrap()).await;
    let before = web_status(&daemon).await["active"].clone();
    let log = send(&daemon).await;
    assert!(log.iter().any(|entry| matches!(&entry.body, EntryBody::ToolCallSettled { outcome: Settlement::Failed { failure }, .. } if failure.code == code::TOOL_FAILED && failure.message.contains("503"))));
    assert_eq!(web_status(&daemon).await["active"], before);
    daemon.stop().await;
}

#[tokio::test]
#[ignore = "calls real web services; set EXA_KEY and TINYFISH_KEY explicitly"]
async fn one_reply_dispatches_three_real_web_calls_as_one_batch() {
    let model = MockServer::start().await;
    embeddings(&model).await;
    Mock::given(path("/chat/completions")).respond_with(|request: &wiremock::Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        if body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "tool") {
            return response(json!({"role":"assistant", "content":"done"}), "stop");
        }
        let calls = [
            tool_message("jina", "web_fetch", json!({"url":"https://example.com"})),
            tool_message("exa", "web_search", json!({"query":"Rust programming language official documentation", "engine":"exa", "limit":2})),
            tool_message("tinyfish", "web_search", json!({"query":"Rust programming language official documentation", "engine":"tinyfish", "limit":2})),
        ].into_iter().map(|message| message["tool_calls"][0].clone()).collect::<Vec<_>>();
        response(json!({"role":"assistant", "content":null, "tool_calls":calls}), "tool_calls")
    }).mount(&model).await;
    let mut document: Value = toml::from_str(&Daemon::config(&model, "openai-compatible")).unwrap();
    document["plugin"] = json!({"web": {"config": {"search": ["exa", "tinyfish"]}, "secrets": {"exa":"EXA_KEY", "tinyfish":"TINYFISH_KEY"}}});
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let began = std::time::Instant::now();
    let log = send(&daemon).await;
    println!(
        "one reply, three real web calls: {:.2}s",
        began.elapsed().as_secs_f64()
    );
    let facts: Vec<_> = log
        .iter()
        .filter(|entry| {
            matches!(
                entry.body,
                EntryBody::ToolCallStarted { .. } | EntryBody::ToolCallSettled { .. }
            )
        })
        .collect();
    assert_eq!(facts.len(), 6);
    assert!(
        facts[..3]
            .iter()
            .all(|entry| matches!(entry.body, EntryBody::ToolCallStarted { .. }))
    );
    assert!(facts[3..].iter().all(|entry| matches!(
        entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            ..
        }
    )));
    daemon.stop().await;
}

async fn propose(model: &MockServer, message: Value) {
    Mock::given(path("/chat/completions"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                response(json!({"role":"assistant", "content":"done"}), "stop")
            } else {
                response(message.clone(), "tool_calls")
            }
        })
        .mount(model)
        .await;
}

#[tokio::test]
async fn required_secret_mapping_is_validated_when_loading_content() {
    let error = WasmRuntime::new()
        .unwrap()
        .load(
            include_bytes!(env!("ENCO_FACTORY_WEB")),
            &PluginSettings {
                config: json!({"search":["exa"]}),
                secrets: Default::default(),
            },
        )
        .await
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("required credential exa has no secret mapping")
    );
}

#[tokio::test]
async fn malformed_jina_content_does_not_trigger_exa_fallback() {
    let services = Services::new().await;
    Mock::given(path("/https://news.test/story"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not JSON</html>"))
        .mount(&services.jina)
        .await;
    propose(
        &services.model,
        tool_message(
            "fetch",
            "web_fetch",
            json!({"url":"https://news.test/story"}),
        ),
    )
    .await;
    let mut daemon =
        Daemon::start_config(&toml::to_string(&services.document(&["exa"])).unwrap()).await;
    let log = send(&daemon).await;
    assert!(log.iter().any(|entry| matches!(&entry.body, EntryBody::ToolCallSettled { outcome: Settlement::Failed { failure }, .. } if failure.message.contains("response is not JSON"))));
    assert!(services.exa.received_requests().await.unwrap().is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn deploying_web_through_the_tool_preserves_its_exact_declared_definitions() {
    let services = Services::new().await;
    let document = services.document(&["exa", "tinyfish"]);
    let settings: PluginSettings =
        serde_json::from_value(document["plugin"]["web"].clone()).unwrap();
    let expected = WasmRuntime::new()
        .unwrap()
        .load(include_bytes!(env!("ENCO_FACTORY_WEB")), &settings)
        .await
        .unwrap()
        .tools
        .unwrap()
        .specs;
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let previous = web_status(&daemon).await["active"]["id"].clone();
    propose(
        &services.model,
        tool_message(
            "deploy",
            "plugin_deploy",
            json!({"name":"web", "generation":previous}),
        ),
    )
    .await;
    let log = send(&daemon).await;
    assert!(log.iter().any(|entry| matches!(
        entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Ok,
            ..
        }
    )));
    assert_ne!(web_status(&daemon).await["active"]["id"], previous);
    let inspection: Inspection = serde_json::from_value(
        daemon
            .connect()
            .await
            .request(Command::Inspect {
                session: "main".into(),
                attempt_id: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    let actual: Vec<_> = inspection
        .plan
        .tools
        .into_iter()
        .map(|(_, spec)| spec)
        .filter(|spec| spec.name.starts_with("web_"))
        .collect();
    assert_eq!(actual, expected);
    let search = actual
        .iter()
        .find(|spec| spec.name == "web_search")
        .unwrap();
    assert_eq!(
        search.input_schema["properties"]["engine"]["enum"],
        json!(["exa", "tinyfish"])
    );
    daemon.stop().await;
}
