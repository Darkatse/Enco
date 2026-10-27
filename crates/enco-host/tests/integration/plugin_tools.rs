#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support;

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
use tokio_util::sync::CancellationToken;

fn spec(name: &str, label: &str, effect: Effect) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("Fixture {label}"),
        input_schema: InputSchema::parse(&json!({
            "type": "object",
            "properties": {"label": {"type": "string"}},
            "additionalProperties": false,
        }))
        .unwrap(),
        effect,
    }
}

struct ConfiguredRuntime {
    provider: Arc<ScriptedProvider>,
}

#[async_trait]
impl Runtime for ConfiguredRuntime {
    async fn load(&self, bytes: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError> {
        if bytes == COMPOSER_BYTES {
            return Ok(composer_loaded(Arc::new(TestComposer)));
        }
        let label = settings.config["label"]
            .as_str()
            .unwrap_or("factory")
            .to_owned();
        let mut loaded = fixture(self.provider.clone(), self.provider.clone());
        // Settings may map `service`, so the component declares it at a fixed origin.
        loaded.credentials = vec![Credential {
            placements: vec![Placement {
                origin: Some("https://service.test".into()),
                location: Location::Header(HeaderPlacement {
                    name: "authorization".into(),
                    prefix: "Bearer ".into(),
                }),
            }],
            ..credential(false)
        }];
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
                images: Vec::new(),
                outgoing: None,
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

async fn tool_kernel(root: &Path, loaded: Loaded) -> Kernel {
    let (development, registry, store) =
        scripted_home(root, factory_runtime(loaded), configuration(), vec![]).await;
    Kernel::start(kernel_deps(root, store, registry, None, Some(development)))
        .await
        .unwrap()
}

async fn send(kernel: &Kernel) -> Vec<Entry> {
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "work".into(), Vec::new())
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
async fn configuration_changes_tools_at_the_next_round_and_rewind_restores_settings() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("fixture_read", "{}"),
            call("self_apply", r#"{"reason":"Apply test intent"}"#),
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
    let (development, registry, store) =
        scripted_home(root.path(), runtime, document.clone(), vec![]).await;
    let initial = registry
        .registered_intent(None)
        .await
        .unwrap()
        .registration()
        .unwrap();
    let old = plugin_generation(&registry);
    document["plugin"]["fixture"] =
        json!({"config": {"label": "new"}, "secrets": {"service": "NEW_KEY"}});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry.clone(),
        None,
        Some(development.clone()),
    ))
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
    for (request, expected) in provider
        .requests
        .lock()
        .unwrap()
        .iter()
        .zip(["Fixture old", "Fixture new"])
    {
        assert_eq!(
            request
                .tools
                .iter()
                .find(|tool| tool.name == "fixture_read")
                .unwrap()
                .description,
            expected
        );
    }
    restore_config(initial, &registry, &development).await;
    let status = registry.status().await.unwrap();
    let restored = running_generation(&status.plugins[0]).unwrap();
    assert_eq!(restored.settings.config, json!({"label":"old"}));
    assert_eq!(restored.settings.secrets["service"], "OLD_KEY");
    assert_eq!(
        status.plugins[0].standing,
        Standing::Registered {
            differences: vec![]
        }
    );
    kernel.shutdown().await.unwrap();
}

async fn restore_config(
    registration: RegistrationId,
    registry: &Registry,
    development: &development::Development,
) {
    development
        .rewind(
            registry,
            development::RewindArguments {
                registration,
                paths: Some(vec![IntentPath::Configuration]),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
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
    let runtime = Arc::new(ConfiguredRuntime { provider });
    let (development, registry, store) =
        scripted_home(root.path(), runtime, configuration(), vec![]).await;
    let factory = plugin_generation(&registry);
    let broken = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"trap",
    )
    .await
    .generations
    .remove(0)
    .id;
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry.clone(),
        None,
        Some(development),
    ))
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
            images: Vec::new(),
            outgoing: None,
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
    let kernel = tool_kernel(root.path(), loaded).await;
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
            images: Vec::new(),
            outgoing: None,
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
    let kernel = tool_kernel(root.path(), loaded).await;
    let session = kernel.open_session("main").await.unwrap();
    kernel
        .submit(session.id, EventId::new(), "work".into(), Vec::new())
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
async fn registrations_and_configuration_reject_native_and_plugin_tool_name_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ConfiguredRuntime {
        provider: ScriptedProvider::new(vec![]),
    });
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let registry = open_registry(
        registry_deps(store.clone(), runtime, FACTORY_BYTES),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    let mut document = configuration();
    document["plugin"] = json!({"fixture": {"config": {"tool":"fs_read"}}});
    let before = store.registry().await.unwrap();
    let host = |plugin: &str, tool: &str| (plugin.parse().unwrap(), tool.into(), ToolOwner::Host);
    assert_eq!(
        tool_conflicts(submit(&registry, configuration_request(document.clone())).await),
        [host("fixture", "fs_read")]
    );
    assert_eq!(
        tool_conflicts(submit(&registry, source_request("native-copy", b"native")).await),
        [host("native-copy", "fs_read")]
    );
    assert_eq!(
        store.registry().await.unwrap().generations,
        before.generations
    );
    assert_eq!(
        store.registry().await.unwrap().config_generations,
        before.config_generations
    );
    submit(&registry, source_request("other", b"other"))
        .await
        .unwrap();
    document["plugin"]["fixture"]["config"]["tool"] = "other_read".into();
    assert_eq!(
        tool_conflicts(submit(&registry, configuration_request(document)).await),
        [(
            "fixture".parse().unwrap(),
            "other_read".into(),
            ToolOwner::Plugins(vec!["other".parse().unwrap()])
        )]
    );
    let exporters: [PluginName; 2] = ["other".parse().unwrap(), "third".parse().unwrap()];
    let conflicts = tool_conflicts(submit(&registry, source_request("third", b"other")).await);
    assert!(!conflicts.is_empty());
    for (plugin, tool, with) in conflicts {
        assert_eq!(tool, "other_read");
        assert!(exporters.contains(&plugin));
        assert!(matches!(with, ToolOwner::Plugins(others)
            if others.iter().all(|other| exporters.contains(other) && *other != plugin)));
    }
}

#[expect(
    clippy::panic,
    reason = "any other outcome fails the test with what was returned"
)]
fn tool_conflicts(
    result: Result<Registered, RegistryError>,
) -> Vec<(PluginName, String, ToolOwner)> {
    let Err(RegistryError::Rejected(causes)) = result else {
        panic!("expected the registration to be rejected");
    };
    causes
        .into_iter()
        .map(|cause| match cause {
            Cause::ToolConflict { plugin, tool, with } => (plugin, tool, with),
            other => panic!("expected only tool conflicts, got {other}"),
        })
        .collect()
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
    let (development, registry, store) =
        scripted_home(root.path(), runtime.clone(), configuration(), vec![]).await;
    let register = async |name: &str, bytes: &[u8]| {
        source_fixture(root.path(), &store, &development, &registry, name, bytes)
            .await
            .generations
            .remove(0)
    };
    let first = register("first", b"shared").await;
    for _ in 0..TRIAL_CALLS {
        registry.report(first.id, Verdict::Ok, None).await.unwrap();
    }
    let renamed = register("first", b"renamed").await;
    let second = register("second", b"shared").await;
    registry
        .report(renamed.id, Verdict::Failed(fault()), None)
        .await
        .unwrap();
    drop(registry);
    let (development, registry) = open_home(
        root.path(),
        store.clone(),
        registry_deps(store.clone(), runtime, FACTORY_BYTES),
        configuration(),
    )
    .await;
    let status = registry.status().await.unwrap();
    for (name, other, generation) in [
        ("first", "second", first.id),
        ("second", "first", second.id),
    ] {
        let plugin = status
            .plugins
            .iter()
            .find(|plugin| plugin.name.as_str() == name)
            .unwrap();
        let Running::Unavailable {
            activation, causes, ..
        } = &plugin.running
        else {
            panic!("{name} should be selected but unavailable");
        };
        assert!(matches!(causes.as_slice(),
            [Cause::ToolConflict { tool, with: ToolOwner::Plugins(others), .. }]
            if tool == "shared_read" && others.iter().map(PluginName::as_str).eq([other])));
        assert_eq!(activation.generation.id, generation);
    }
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry,
        None,
        Some(development),
    ))
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

#[tokio::test]
async fn self_apply_moves_the_next_attempt_and_memory_call_to_the_restored_code() {
    let root = tempfile::tempdir().unwrap();
    let previous = ScriptedProvider::new(vec![calls(vec![call(
        "self_apply",
        r#"{"reason":"Apply test intent"}"#,
    )])]);
    let restored = ScriptedProvider::new(vec![reply("factory code")]);
    *restored.embedding_failure.lock().unwrap() = Some(Failure {
        code: code::PROVIDER_NETWORK.into(),
        message: "offline".into(),
        retryable: true,
    });
    let runtime = factory_runtime(fixture(restored.clone(), restored.clone()));
    runtime.insert(b"source", fixture(previous.clone(), previous));
    let mut document = configuration();
    for name in ["reply", "compaction"] {
        document["endpoint"][name]["model"] = "per-call-model".into();
    }
    let (development, registry, store) =
        scripted_home(root.path(), runtime, document, vec![]).await;
    let previous = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"source",
    )
    .await
    .generations[0]
        .id;
    std::fs::remove_dir_all(root.path().join("plugins/fixture")).unwrap();
    let settings = registry.view().config.kernel.profiles["default"]
        .reply
        .first()
        .endpoint
        .settings
        .clone();
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry.clone(),
        None,
        Some(development),
    ))
    .await
    .unwrap();
    let memories = open_memories(root.path(), registry.clone()).await;
    let entries = send(&kernel).await;
    let starts: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::AttemptStarted {
                provider, settings, ..
            } => Some((provider, settings)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0].0, &previous);
    assert_eq!(*starts[1].0, plugin_generation(&registry));
    assert_ne!(starts[0].0, starts[1].0);
    assert!(starts.iter().all(|(_, selected)| **selected == settings));
    assert_eq!(
        restored.settings.lock().unwrap().as_slice(),
        std::slice::from_ref(&settings)
    );
    assert!(
        memories
            .recall("after", 1, &CancellationToken::new())
            .await
            .unwrap()
            .lexical_only
            .is_some()
    );
    kernel.shutdown().await.unwrap();
}
