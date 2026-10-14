#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use support::*;
use tokio::sync::Barrier;

fn spec(name: &str, label: &str, effect: Effect) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("Fixture {label}"),
        input_schema: closed_object_schema(json!({"label": {"type":"string"}}), &[]),
        effect,
    }
}

struct ConfiguredRuntime {
    provider: Arc<ScriptedProvider>,
}

#[async_trait]
impl Runtime for ConfiguredRuntime {
    async fn load(&self, bytes: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError> {
        let label = settings.config["label"]
            .as_str()
            .unwrap_or("factory")
            .to_owned();
        let mut loaded = fixture(self.provider.clone(), self.provider.clone());
        loaded.tools = Some(LoadedTools {
            specs: vec![spec(
                settings.config["tool"].as_str().unwrap_or(match bytes {
                    b"other" => "other_read",
                    b"native" => "fs_read",
                    _ => "fixture_read",
                }),
                &label,
                Effect::ReadOnly,
            )],
            adapter: Arc::new(Echo {
                label,
                trap: bytes == b"trap",
            }),
        });
        Ok(loaded)
    }
}

struct Echo {
    label: String,
    trap: bool,
}

#[async_trait]
impl Tools for Echo {
    async fn call(
        &self,
        _ctx: CallContext,
        _spec: &ToolSpec,
        _args: serde_json::Map<String, Value>,
    ) -> Outcome {
        if self.trap {
            Outcome::Failed { failure: fault() }
        } else {
            Outcome::Ok {
                value: self.label.clone().into(),
            }
        }
    }
}

fn fault() -> Failure {
    Failure {
        code: code::PLUGIN_TRAP.into(),
        message: "fixture trap".into(),
        retryable: false,
    }
}

async fn configured(
    root: &Path,
    runtime: Arc<dyn Runtime>,
    document: Value,
) -> (Arc<Registry>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime,
        FACTORY_BYTES,
        document,
    ))
    .await
    .unwrap();
    (registry, store)
}

async fn send(kernel: &Kernel) -> Vec<Entry> {
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "work".into())
        .await
        .unwrap();
    finish(&mut rx).await
}

fn plugin_generation(registry: &Registry) -> GenerationId {
    registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), false)
        .unwrap()
        .generation
}

#[tokio::test]
async fn configuration_changes_tools_at_the_next_round_and_redeployment_restores_all_settings() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("fixture_read", "{}"),
            call("config_apply", "{}"),
            call("fixture_read", "{}"),
        ]),
        calls(vec![call("fixture_read", "{}")]),
        reply("done"),
    ]);
    let runtime = Arc::new(ConfiguredRuntime {
        provider: provider.clone(),
    });
    let mut document = configuration();
    document["plugin"] =
        json!({"fixture": {"config": {"label": "old"}, "secrets": {"service": "OLD_KEY"}}});
    let (registry, store) = configured(root.path(), runtime, document.clone()).await;
    let old = plugin_generation(&registry);
    document["plugin"]["fixture"] =
        json!({"config": {"label": "new"}, "secrets": {"service": "NEW_KEY"}});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let kernel = Kernel::start(kernel_deps(root.path(), store, registry.clone(), None))
        .await
        .unwrap();
    let log = send(&kernel).await;
    let new = plugin_generation(&registry);
    assert_ne!(old, new);
    let bound: Vec<_> = log
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallStarted {
                capability, code, ..
            } if capability.name == "fixture_read" => Some(code.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        bound,
        [
            CodeRef::Generation { id: old },
            CodeRef::Generation { id: old },
            CodeRef::Generation { id: new }
        ]
    );
    let outputs: Vec<_> = log
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled { content, .. } if content == "old" || content == "new" => {
                Some(content.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(outputs, ["old", "old", "new"]);
    {
        let requests = provider.requests.lock().unwrap();
        assert_eq!(
            requests[0]
                .tools
                .iter()
                .find(|tool| tool.name == "fixture_read")
                .unwrap()
                .description,
            "Fixture old"
        );
        assert_eq!(
            requests[1]
                .tools
                .iter()
                .find(|tool| tool.name == "fixture_read")
                .unwrap()
                .description,
            "Fixture new"
        );
    }
    let restored = registry
        .deploy("fixture", PluginSource::Generation(old))
        .await
        .unwrap()
        .generation;
    assert_eq!(restored.settings.config, json!({"label":"old"}));
    assert_eq!(restored.settings.secrets["service"], "OLD_KEY");
    assert_eq!(
        registry.status().await.plugins[0].settings_match,
        Some(false)
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_bound_code_stops_later_calls_until_the_next_round() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("fixture_read", "{}"),
            call("fs_write", r#"{"path":"barrier.txt","content":"written"}"#),
            call("fixture_read", "{}"),
        ]),
        calls(vec![call("fixture_read", "{}")]),
        reply("done"),
    ]);
    let (registry, store) = configured(
        root.path(),
        Arc::new(ConfiguredRuntime { provider }),
        configuration(),
    )
    .await;
    let factory = plugin_generation(&registry);
    let broken = registry
        .deploy("fixture", PluginSource::Artifact(b"trap".to_vec()))
        .await
        .unwrap()
        .generation
        .id;
    let kernel = Kernel::start(kernel_deps(root.path(), store, registry.clone(), None))
        .await
        .unwrap();
    let log = send(&kernel).await;
    let failures: Vec<_> = log
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Failed { failure },
                ..
            } => Some(failure),
            _ => None,
        })
        .collect();
    assert_eq!(
        failures
            .iter()
            .map(|failure| failure.code.as_str())
            .collect::<Vec<_>>(),
        [code::PLUGIN_TRAP, code::PLUGIN_ROLLED_BACK]
    );
    assert!(
        failures[1].message.contains(&broken.to_string())
            && failures[1].message.contains(&factory.to_string())
    );
    assert!(log.iter().any(|entry| matches!(&entry.body, EntryBody::ToolCallSettled { content, .. } if content == "factory")));
    assert_eq!(
        log.iter()
            .filter(|entry| matches!(&entry.body,
                EntryBody::ToolCallStarted { code: CodeRef::Generation { id }, .. } if *id == broken
            ))
            .count(),
        1
    );
    assert_eq!(plugin_generation(&registry), factory);
    kernel.shutdown().await.unwrap();
}

struct Ordered {
    pair: Barrier,
    events: Mutex<Vec<String>>,
}

#[async_trait]
impl Tools for Ordered {
    async fn call(
        &self,
        _ctx: CallContext,
        _spec: &ToolSpec,
        args: serde_json::Map<String, Value>,
    ) -> Outcome {
        let label = args["label"].as_str().unwrap();
        self.events.lock().unwrap().push(format!("start:{label}"));
        if matches!(label, "a" | "b") {
            self.pair.wait().await;
        }
        self.events.lock().unwrap().push(format!("end:{label}"));
        Outcome::Ok {
            value: label.into(),
        }
    }
}

#[tokio::test]
async fn readonly_batches_overlap_and_settle_in_proposal_order_around_mutations() {
    let root = tempfile::tempdir().unwrap();
    let ordered = Arc::new(Ordered {
        pair: Barrier::new(2),
        events: Mutex::new(Vec::new()),
    });
    let proposed = vec![
        call("read", r#"{"label":"a"}"#),
        call("read", r#"{"label":"b"}"#),
        call("write", r#"{"label":"w"}"#),
        call("read", r#"{"label":"c"}"#),
    ];
    let ids: Vec<_> = proposed.iter().map(|call| call.id).collect();
    let provider = ScriptedProvider::new(vec![calls(proposed), reply("done")]);
    let mut loaded = fixture(provider.clone(), provider);
    loaded.tools = Some(LoadedTools {
        specs: vec![
            spec("read", "read", Effect::ReadOnly),
            spec("write", "write", Effect::SideEffect),
        ],
        adapter: ordered.clone(),
    });
    let (registry, store) = registry_configured(root.path(), loaded, configuration(), vec![]).await;
    let kernel = Kernel::start(kernel_deps(root.path(), store, registry, None))
        .await
        .unwrap();
    let log = send(&kernel).await;
    let settled: Vec<_> = log
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::ToolCallSettled { call, .. } => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(settled, ids);
    {
        let events = ordered.events.lock().unwrap();
        assert!(events[0].starts_with("start:") && events[1].starts_with("start:"));
        assert_eq!(&events[4..], ["start:w", "end:w", "start:c", "end:c"]);
    }
    kernel.shutdown().await.unwrap();
}

struct Cancelling {
    entered: Barrier,
    cancelled: Barrier,
    finish: Barrier,
}

#[async_trait]
impl Tools for Cancelling {
    async fn call(
        &self,
        ctx: CallContext,
        _spec: &ToolSpec,
        _args: serde_json::Map<String, Value>,
    ) -> Outcome {
        self.entered.wait().await;
        ctx.cancel.cancelled().await;
        self.cancelled.wait().await;
        self.finish.wait().await;
        Outcome::Ok {
            value: json!("quiescent"),
        }
    }
}

#[tokio::test]
async fn shutdown_signals_the_whole_batch_and_waits_for_every_settlement() {
    let root = tempfile::tempdir().unwrap();
    let waiting = Arc::new(Cancelling {
        entered: Barrier::new(3),
        cancelled: Barrier::new(3),
        finish: Barrier::new(3),
    });
    let provider = ScriptedProvider::new(vec![calls(vec![call("wait", "{}"), call("wait", "{}")])]);
    let mut loaded = fixture(provider.clone(), provider);
    loaded.tools = Some(LoadedTools {
        specs: vec![spec("wait", "wait", Effect::ReadOnly)],
        adapter: waiting.clone(),
    });
    let (registry, store) = registry_configured(root.path(), loaded, configuration(), vec![]).await;
    let kernel = Kernel::start(kernel_deps(root.path(), store, registry, None))
        .await
        .unwrap();
    let session = kernel.open_session("main").await.unwrap();
    kernel
        .submit(session.id, EventId::new(), "work".into())
        .await
        .unwrap();
    waiting.entered.wait().await;
    let stopped = tokio::spawn(async move {
        kernel.shutdown().await.unwrap();
        kernel
    });
    waiting.cancelled.wait().await;
    assert!(!stopped.is_finished());
    waiting.finish.wait().await;
    let kernel = stopped.await.unwrap();
    let log = kernel.log(session.id, None).await.unwrap();
    assert_eq!(
        log.iter()
            .filter(|entry| matches!(
                entry.body,
                EntryBody::ToolCallSettled {
                    outcome: Settlement::Ok,
                    ..
                }
            ))
            .count(),
        2
    );
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
}

#[tokio::test]
async fn deployments_and_configuration_reject_native_and_plugin_tool_name_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ConfiguredRuntime {
        provider: ScriptedProvider::new(vec![]),
    });
    let (registry, store) = configured(root.path(), runtime, configuration()).await;
    let mut document = configuration();
    document["plugin"] = json!({"fixture": {"config": {"tool":"fs_read"}}});
    let before = store.registry().await.unwrap();
    let rejected = registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .err()
        .unwrap();
    assert!(rejected.to_string().contains("fs_read") && rejected.to_string().contains("native"));
    assert!(
        registry
            .deploy("native-copy", PluginSource::Artifact(b"native".to_vec()))
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("fs_read")
    );
    assert_eq!(
        store.registry().await.unwrap().generations,
        before.generations
    );
    assert_eq!(
        store.registry().await.unwrap().config_generations,
        before.config_generations
    );
    registry
        .deploy("other", PluginSource::Artifact(b"other".to_vec()))
        .await
        .unwrap();
    document["plugin"]["fixture"]["config"]["tool"] = "other_read".into();
    assert!(
        registry
            .apply_config(ConfigSource::Document(document))
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("other_read")
    );
    assert!(
        registry
            .deploy("third", PluginSource::Artifact(b"other".to_vec()))
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("other_read")
    );
}

#[tokio::test]
async fn mechanical_rollback_excludes_both_conflicts_and_keeps_native_tools_running() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("fs_read", r#"{"path":"repair.txt"}"#)]),
        reply("done"),
    ]);
    runtime.insert(FACTORY_BYTES, fixture(provider.clone(), provider.clone()));
    for (bytes, name) in [
        (b"shared".as_slice(), "shared_read"),
        (b"renamed", "first_read"),
    ] {
        let mut loaded = fixture(provider.clone(), provider.clone());
        loaded.tools = Some(LoadedTools {
            specs: vec![spec(name, name, Effect::ReadOnly)],
            adapter: Arc::new(Echo {
                label: name.into(),
                trap: false,
            }),
        });
        runtime.insert(bytes, loaded);
    }
    let (registry, store) = configured(root.path(), runtime.clone(), configuration()).await;
    let first = registry
        .deploy("first", PluginSource::Artifact(b"shared".to_vec()))
        .await
        .unwrap()
        .generation;
    for _ in 0..TRIAL_CALLS {
        registry.report(first.id, Verdict::Ok, None).await.unwrap();
    }
    registry
        .deploy("first", PluginSource::Artifact(b"renamed".to_vec()))
        .await
        .unwrap();
    let second = registry
        .deploy("second", PluginSource::Artifact(b"shared".to_vec()))
        .await
        .unwrap()
        .generation;
    registry.rollback("first").await.unwrap();
    drop(registry);
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime,
        FACTORY_BYTES,
        configuration(),
    ))
    .await
    .unwrap();
    let status = registry.status().await;
    for name in ["first", "second"] {
        let plugin = status
            .plugins
            .iter()
            .find(|plugin| plugin.name == name)
            .unwrap();
        let reason = plugin.unavailable.as_ref().unwrap();
        assert!(reason.contains("shared_read") && reason.contains("first, second"));
        assert_eq!(
            plugin.active.as_ref().unwrap().id,
            if name == "first" { first.id } else { second.id }
        );
    }
    let kernel = Kernel::start(kernel_deps(root.path(), store, registry, None))
        .await
        .unwrap();
    std::fs::write(root.path().join("workspace/repair.txt"), "repair available").unwrap();
    let log = send(&kernel).await;
    assert!(log.iter().any(|entry| matches!(&entry.body, EntryBody::ToolCallSettled { outcome: Settlement::Ok, content, .. } if content.contains("repair available"))));
    assert!(
        provider.requests.lock().unwrap()[0]
            .tools
            .iter()
            .all(|spec| spec.name != "shared_read")
    );
    kernel.shutdown().await.unwrap();
}
