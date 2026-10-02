#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::sync::{Arc, Mutex, atomic::Ordering};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn loaded(provider: Arc<ScriptedProvider>) -> Loaded {
    Loaded {
        lifecycle: Arc::new(Probe(Ok(()))),
        summary: "fixture".into(),
        completion: Some(provider.clone()),
        embedding: Some(provider),
    }
}

async fn open(
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    factory: &[u8],
    wiring: Vec<Use>,
) -> Result<Arc<Registry>, RegistryError> {
    Registry::open(RegistryDeps {
        store,
        runtime,
        factory: vec![FactoryPlugin {
            name: "fixture".into(),
            id: FIXTURE_ID.parse().unwrap(),
            artifact: factory.to_vec(),
        }],
        wiring,
        clock: Arc::new(SystemClock),
    })
    .await
}

async fn promote(registry: &Registry, generation: GenerationId) {
    for _ in 0..TRIAL_CALLS {
        registry
            .report(generation, Verdict::Ok, None)
            .await
            .unwrap();
    }
}

fn fault(code: &str) -> Failure {
    Failure {
        code: code.into(),
        message: "fixture failure".into(),
        retryable: false,
    }
}

#[tokio::test]
async fn factory_updates_are_idempotent_and_preserve_owner_deployments() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in [b"factory-1", b"factory-2", b"factory-3"] {
        runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let registry = open(store.clone(), runtime.clone(), b"factory-1", vec![])
        .await
        .unwrap();
    let first = registry.status().await.remove(0);
    drop(registry);
    let registry = open(store.clone(), runtime.clone(), b"factory-1", vec![])
        .await
        .unwrap();
    assert_eq!(registry.status().await[0].generations, first.generations);
    drop(registry);
    let registry = open(store.clone(), runtime.clone(), b"factory-2", vec![])
        .await
        .unwrap();
    let second = registry.status().await.remove(0);
    assert_eq!(second.generations.len(), 2);
    assert!(second.active.unwrap().id > first.active.unwrap().id);
    let deployed = registry
        .deploy("fixture", b"factory-1".to_vec())
        .await
        .unwrap()
        .generation;
    drop(registry);
    let registry = open(store.clone(), runtime.clone(), b"factory-3", vec![])
        .await
        .unwrap();
    assert_eq!(registry.status().await[0].active.as_ref(), Some(&deployed));
    assert_eq!(registry.status().await[0].generations.len(), 4);
    drop(registry);
    std::fs::write(
        dir.path().join("plugins.lock"),
        format!("[plugins.fixture]\nid = \"{}\"\n", PluginId::new()),
    )
    .unwrap();
    assert!(matches!(
        open(store, runtime, b"factory-3", vec![]).await,
        Err(RegistryError::FactoryIdentity(_))
    ));
}

#[tokio::test]
async fn admission_keeps_configured_exports_connected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    runtime.insert(FACTORY_BYTES, loaded(provider.clone()));
    runtime.insert(
        b"completion-only",
        Loaded {
            lifecycle: Arc::new(Probe(Ok(()))),
            summary: "completion".into(),
            completion: Some(provider),
            embedding: None,
        },
    );
    let wiring = vec![Use {
        user: "embedding".into(),
        plugin: "fixture".into(),
        interface: Interface::Embedding,
    }];
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, wiring)
        .await
        .unwrap();
    let before = registry.status().await.remove(0);
    assert!(
        matches!(registry.deploy("fixture", b"completion-only".to_vec()).await,
        Err(RegistryError::Rejected { reason, .. }) if reason.contains("embedding"))
    );
    assert_eq!(registry.status().await[0].generations, before.generations);
    drop(registry);
    let wiring = vec![Use {
        user: "profile default.reply".into(),
        plugin: "missing".into(),
        interface: Interface::Completion,
    }];
    assert!(open(store, runtime, FACTORY_BYTES, wiring).await.is_err());
}

#[tokio::test]
async fn recovery_uses_committed_activation_and_skips_missing_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"older", b"latest"] {
        runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let factory = registry.status().await[0].active.as_ref().unwrap().id;
    let older = registry
        .deploy("fixture", b"older".to_vec())
        .await
        .unwrap()
        .generation;
    promote(&registry, older.id).await;
    // Emulate a crash after the durable commit but before in-memory publication.
    let pending = NewGeneration {
        plugin: older.plugin,
        artifact: store.put_artifact(b"latest").await.unwrap(),
        config: serde_json::json!({}),
        origin: Origin::Deployed,
        status: GenerationStatus::Healthy,
        created_at: SystemClock.now().to_utc(),
    };
    let latest = store.insert_generation(&pending, true).await.unwrap();
    drop(registry);
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    assert_eq!(
        registry
            .exports()
            .completion("fixture", false)
            .unwrap()
            .generation,
        latest
    );
    drop(registry);
    std::fs::remove_file(
        dir.path()
            .join("artifacts")
            .join(format!("{}.wasm", pending.artifact)),
    )
    .unwrap();
    std::fs::write(
        dir.path()
            .join("artifacts")
            .join(format!("{}.wasm", older.artifact)),
        b"corrupt",
    )
    .unwrap();
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let status = registry.status().await.remove(0);
    assert_eq!(status.active.as_ref().unwrap().id, factory);
    for record in status
        .generations
        .iter()
        .filter(|record| record.id != factory)
    {
        assert_eq!(record.status, GenerationStatus::Failed);
        let failure = record.failure.as_ref().unwrap();
        assert_eq!(failure.code, code::PLUGIN_LOAD);
        assert!(failure.message.contains(&record.artifact.to_string()));
    }

    let only = registry
        .deploy("custom", b"latest".to_vec())
        .await
        .unwrap()
        .generation;
    promote(&registry, only.id).await;
    let newer = registry
        .deploy("custom", b"older".to_vec())
        .await
        .unwrap()
        .generation;
    assert_eq!(store.artifact(&newer.artifact).await.unwrap(), b"older");
    std::fs::remove_file(
        dir.path()
            .join("artifacts")
            .join(format!("{}.wasm", only.artifact)),
    )
    .unwrap();
    assert!(matches!(
        registry.rollback("custom").await,
        Err(RegistryError::NoRollbackTarget(_))
    ));
    assert_eq!(
        registry
            .exports()
            .completion("custom", false)
            .unwrap()
            .generation,
        newer.id
    );
    drop(registry);
    std::fs::remove_file(
        dir.path()
            .join("artifacts")
            .join(format!("{}.wasm", newer.artifact)),
    )
    .unwrap();
    // Both custom artifacts are now unavailable; recovery leaves it registered without exports.
    let registry = open(store, runtime, FACTORY_BYTES, vec![]).await.unwrap();
    assert_eq!(
        registry
            .exports()
            .completion("custom", false)
            .err()
            .unwrap()
            .code,
        code::PLUGIN_UNAVAILABLE
    );
    let restored = registry.status().await;
    assert_eq!(
        restored
            .iter()
            .find(|plugin| plugin.name == "fixture")
            .unwrap()
            .generations,
        status.generations
    );
    let custom = restored
        .iter()
        .find(|plugin| plugin.name == "custom")
        .unwrap();
    for record in &custom.generations {
        assert_eq!(record.failure.as_ref().unwrap().code, code::PLUGIN_LOAD);
    }
}

struct PausedLoad {
    runtime: Arc<ScriptedRuntime>,
    pause: Mutex<Option<ContentHash>>,
    entered: Notify,
    resume: Notify,
}

#[async_trait]
impl Runtime for PausedLoad {
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError> {
        let result = self.runtime.load(artifact, config).await;
        let pause = self
            .pause
            .lock()
            .unwrap()
            .take_if(|hash| *hash == ContentHash::of(artifact))
            .is_some();
        if pause {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        result
    }
}

#[tokio::test]
async fn rollback_cannot_overwrite_a_deployment_committed_during_loading() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let scripted = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"one", b"two", b"three", b"four"] {
        scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let runtime = Arc::new(PausedLoad {
        runtime: scripted,
        pause: Mutex::new(None),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let registry = open(store, runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let one = registry
        .deploy("fixture", b"one".to_vec())
        .await
        .unwrap()
        .generation;
    promote(&registry, one.id).await;
    let two = registry
        .deploy("fixture", b"two".to_vec())
        .await
        .unwrap()
        .generation;
    promote(&registry, two.id).await;
    *runtime.pause.lock().unwrap() = Some(one.artifact);
    let rollback = tokio::spawn({
        let registry = registry.clone();
        async move { registry.rollback("fixture").await }
    });
    runtime.entered.notified().await;
    let (three, four) = tokio::join!(
        registry.deploy("fixture", b"three".to_vec()),
        registry.deploy("fixture", b"four".to_vec())
    );
    let three = three.unwrap().generation;
    let four = four.unwrap().generation;
    runtime.resume.notify_one();
    assert!(matches!(
        rollback.await.unwrap(),
        Err(RegistryError::Conflict(_))
    ));
    let status = registry.status().await.remove(0);
    assert_eq!(status.generations.len(), 5);
    assert_eq!(status.active.unwrap().id, three.id.max(four.id));
    let previous = registry.rollback("fixture").await.unwrap();
    assert_eq!(previous.id, two.id);
}

#[tokio::test]
async fn deployment_tool_changes_the_next_attempt_and_memory_call() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("new.wasm"), b"new").unwrap();
    let old = ScriptedProvider::new(vec![calls(vec![call(
        "plugin_deploy",
        r#"{"name":"fixture","path":"new.wasm"}"#,
    )])]);
    let new = ScriptedProvider::new(vec![reply("new code")]);
    *new.embedding_failure.lock().unwrap() = Some(fault(code::PROVIDER_NETWORK));
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(old.clone()));
    runtime.insert(b"new", loaded(new.clone()));
    let registry = open(store.clone(), runtime, FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let factory = registry
        .exports()
        .completion("fixture", false)
        .unwrap()
        .generation;
    let mut endpoint = endpoint();
    endpoint.settings.model = "per-call-model".into();
    let mut tools = native_tools(workspace.clone());
    tools.extend(plugin_tools(registry.clone(), workspace.clone()));
    let kernel = Kernel::start(
        KernelDeps {
            store,
            registry: registry.clone(),
            profiles: [(
                "default".into(),
                Profile {
                    reply: endpoint.clone(),
                    compaction: endpoint.clone(),
                    requires_lifeline: true,
                },
            )]
            .into(),
            composer: Arc::new(FactoryComposer::new(
                workspace,
                dir.path().join("AGENTS.md"),
            )),
            context: vec![],
            tools,
            lifeline: LIFELINE.iter().map(|name| (*name).into()).collect(),
            clock: Arc::new(SystemClock),
        },
        KernelConfig::new(24).unwrap(),
    )
    .await
    .unwrap();
    let memories = Memories::open(
        MemoryPaths {
            db: dir.path().join("memory.db"),
            index: dir.path().join("memory-index"),
        },
        embedding_endpoint("fixture", 64),
        registry.clone(),
        Arc::new(SystemClock),
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "replace your provider".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
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
    assert_eq!(starts[0].0, &CodeRef::Generation { id: factory });
    assert_eq!(
        starts[1].0,
        &CodeRef::Generation {
            id: registry
                .exports()
                .completion("fixture", false)
                .unwrap()
                .generation
        }
    );
    assert!(
        starts
            .iter()
            .all(|(_, settings)| **settings == endpoint.settings)
    );
    assert_eq!(
        new.settings.lock().unwrap().as_slice(),
        &[endpoint.settings]
    );
    assert!(
        memories
            .recall("after", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .is_some()
    );
    new.steps.lock().unwrap().push_back(calls(vec![call(
        "plugin_rollback",
        r#"{"name":"fixture"}"#,
    )]));
    old.steps.lock().unwrap().push_back(reply("restored"));
    kernel
        .submit(session.id, EventId::new(), "restore your provider".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let returned = entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Ok,
                content,
                ..
            } => Some(serde_json::from_str::<GenerationRecord>(content).unwrap()),
            _ => None,
        })
        .unwrap();
    assert_eq!(returned.id, factory);
    assert!(
        memories
            .recall("restored", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .is_none()
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn trial_health_and_notices_follow_durable_registry_commits() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    runtime.insert(FACTORY_BYTES, loaded(provider.clone()));
    let mut rejected = loaded(provider);
    rejected.lifecycle = Arc::new(Probe(Err(fault(code::PLUGIN_TRAP))));
    runtime.insert(b"rejected", rejected);
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let before = store.registry().await.unwrap().generations;
    assert!(matches!(
        registry.deploy("new-name", b"rejected".to_vec()).await,
        Err(RegistryError::Rejected { .. })
    ));
    assert_eq!(store.registry().await.unwrap().generations, before);
    assert!(!store.plugin_names().await.unwrap().contains_key("new-name"));

    let trial = registry
        .deploy("custom", FACTORY_BYTES.to_vec())
        .await
        .unwrap()
        .generation;
    for _ in 0..TRIAL_CALLS - 1 {
        registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    }
    drop(registry);
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    for _ in 0..TRIAL_CALLS - 1 {
        registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    }
    for code in [
        code::PROVIDER_NETWORK,
        code::TIMEOUT,
        code::CANCELLED,
        code::PLUGIN_UNAVAILABLE,
    ] {
        registry
            .report(trial.id, Verdict::Failed(fault(code)), None)
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .registry()
            .await
            .unwrap()
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Trial
    );
    registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    assert_eq!(
        store
            .registry()
            .await
            .unwrap()
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Healthy
    );
    assert!(
        registry
            .report(trial.id, Verdict::Failed(fault(code::PLUGIN_TRAP)), None)
            .await
            .unwrap()
            .is_none()
    );

    let next = registry
        .deploy("custom", FACTORY_BYTES.to_vec())
        .await
        .unwrap()
        .generation;
    let session = store.ensure_session("notice", Utc::now()).await.unwrap();
    let sql = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON inbox BEGIN SELECT RAISE(ABORT, 'notice unavailable'); END;").unwrap();
    assert!(
        registry
            .report(
                next.id,
                Verdict::Failed(fault(code::PLUGIN_TRAP)),
                Some(session.id)
            )
            .await
            .is_err()
    );
    assert_eq!(
        registry
            .exports()
            .completion("custom", false)
            .unwrap()
            .generation,
        next.id
    );
    assert_eq!(
        store.registry().await.unwrap().generations.last().unwrap(),
        &next
    );
    assert!(store.pending(session.id).await.unwrap().is_empty());
    sql.execute_batch("DROP TRIGGER reject_notice").unwrap();
    let rollback = registry
        .report(
            next.id,
            Verdict::Failed(fault(code::PLUGIN_TRAP)),
            Some(session.id),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rollback.to, Some(trial.id));
    assert!(
        registry
            .report(
                next.id,
                Verdict::Failed(fault(code::PLUGIN_TRAP)),
                Some(session.id)
            )
            .await
            .unwrap()
            .is_none()
    );
    drop(registry);
    drop(store);
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let registry = open(store.clone(), runtime, FACTORY_BYTES, vec![])
        .await
        .unwrap();
    assert_eq!(
        registry
            .exports()
            .completion("custom", false)
            .unwrap()
            .generation,
        trial.id
    );
    let status = registry.status().await;
    let failed = status
        .iter()
        .find(|plugin| plugin.name == "custom")
        .unwrap()
        .generations
        .last()
        .unwrap();
    assert_eq!(failed.status, GenerationStatus::Failed);
    assert_eq!(failed.failure.as_ref(), Some(&rollback.failure));
    let pending = store.pending(session.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].body, EventBody::GenerationRolledBack(rollback));

    let alone = registry
        .deploy("alone", FACTORY_BYTES.to_vec())
        .await
        .unwrap()
        .generation;
    assert_eq!(
        registry
            .report(
                alone.id,
                Verdict::Failed(fault(code::PLUGIN_CONTRACT)),
                None
            )
            .await
            .unwrap()
            .unwrap()
            .to,
        None
    );
    assert_eq!(
        registry
            .exports()
            .completion("alone", false)
            .err()
            .unwrap()
            .code,
        code::PLUGIN_UNAVAILABLE
    );
}

#[tokio::test]
async fn automatic_rollback_discards_preparation_after_deployment_or_promotion() {
    for promote_current in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
        let scripted = Arc::new(ScriptedRuntime::default());
        for bytes in [FACTORY_BYTES, b"healthy", b"trial"] {
            scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
        }
        let runtime = Arc::new(PausedLoad {
            runtime: scripted,
            pause: Mutex::new(None),
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
            .await
            .unwrap();
        let healthy = registry
            .deploy("fixture", b"healthy".to_vec())
            .await
            .unwrap()
            .generation;
        promote(&registry, healthy.id).await;
        let trial = registry
            .deploy("fixture", b"trial".to_vec())
            .await
            .unwrap()
            .generation;
        *runtime.pause.lock().unwrap() = Some(healthy.artifact);
        let session = store.ensure_session("notice", Utc::now()).await.unwrap();
        let report = tokio::spawn({
            let registry = registry.clone();
            async move {
                registry
                    .report(
                        trial.id,
                        Verdict::Failed(fault(code::PLUGIN_TRAP)),
                        Some(session.id),
                    )
                    .await
            }
        });
        runtime.entered.notified().await;
        let expected = if promote_current {
            promote(&registry, trial.id).await;
            trial.id
        } else {
            registry
                .deploy("fixture", b"trial".to_vec())
                .await
                .unwrap()
                .generation
                .id
        };
        runtime.resume.notify_one();
        assert!(report.await.unwrap().unwrap().is_none());
        assert_eq!(
            registry
                .exports()
                .completion("fixture", false)
                .unwrap()
                .generation,
            expected
        );
        assert!(
            store
                .registry()
                .await
                .unwrap()
                .generations
                .iter()
                .all(|record| record.status != GenerationStatus::Failed && record.failure.is_none())
        );
        assert!(store.pending(session.id).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn callers_share_trial_health_and_recovery_keeps_the_recorded_request() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let factory = ScriptedProvider::new(vec![reply("safe mode")]);
    let provider = ScriptedProvider::new(vec![reply("active"), reply("healthy")]);
    let broken = ScriptedProvider::new(vec![Err(fault(code::PLUGIN_TRAP))]);
    runtime.insert(FACTORY_BYTES, loaded(factory.clone()));
    runtime.insert(b"working", loaded(provider.clone()));
    runtime.insert(b"broken", loaded(broken.clone()));
    let registry = open(store.clone(), runtime.clone(), FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let factory_id = registry
        .exports()
        .completion("fixture", true)
        .unwrap()
        .generation;
    let trial = registry
        .deploy("fixture", b"working".to_vec())
        .await
        .unwrap()
        .generation;
    let mut settings_profile = profile();
    settings_profile.requires_lifeline = false;
    settings_profile.reply.settings.model = "chosen-model".into();
    let kernel = Kernel::start(
        KernelDeps {
            store: store.clone(),
            registry: registry.clone(),
            profiles: [("default".into(), settings_profile.clone())].into(),
            composer: Arc::new(FactoryComposer::new(
                dir.path().into(),
                dir.path().join("AGENTS.md"),
            )),
            context: vec![],
            tools: vec![],
            lifeline: vec![],
            clock: Arc::new(SystemClock),
        },
        KernelConfig::new(24).unwrap(),
    )
    .await
    .unwrap();
    let memories = Memories::open(
        MemoryPaths {
            db: dir.path().join("memory.db"),
            index: dir.path().join("memory-index"),
        },
        embedding_endpoint("fixture", 64),
        registry.clone(),
        Arc::new(SystemClock),
    )
    .await
    .unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    for (safe, expected) in [(true, factory_id), (false, trial.id)] {
        kernel.set_safe_mode(safe).await.unwrap();
        kernel
            .submit(session.id, EventId::new(), "reply".into())
            .await
            .unwrap();
        let entries = finish(&mut rx).await;
        let attempt = entries
            .iter()
            .find_map(|entry| match entry.body {
                EntryBody::AttemptStarted {
                    attempt,
                    provider: CodeRef::Generation { id },
                    ..
                } => {
                    assert_eq!(id, expected);
                    Some(attempt)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            kernel
                .inspect(session.id, Some(attempt))
                .await
                .unwrap()
                .settings,
            settings_profile.reply.settings
        );
    }
    // An index configuration mismatch follows a valid plugin result and still counts.
    provider.embedding_dimensions.store(32, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    for _ in 0..TRIAL_CALLS - 2 {
        assert!(
            memories
                .recall("memory", 1, &cancel)
                .await
                .unwrap()
                .lexical_only
                .is_some()
        );
    }
    assert_eq!(
        registry.status().await[0].active.as_ref().unwrap().status,
        GenerationStatus::Trial
    );
    kernel
        .submit(session.id, EventId::new(), "one more success".into())
        .await
        .unwrap();
    finish(&mut rx).await;
    assert_eq!(
        registry.status().await[0].active.as_ref().unwrap().status,
        GenerationStatus::Healthy
    );

    *broken.embedding_failure.lock().unwrap() = Some(fault(code::PLUGIN_CONTRACT));
    let embedding_id = registry
        .deploy("fixture", b"broken".to_vec())
        .await
        .unwrap()
        .generation
        .id;
    assert_eq!(
        memories
            .recall("failed embedding", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .unwrap()
            .code,
        code::PLUGIN_CONTRACT
    );
    assert_eq!(
        registry.exports().embedding("fixture").unwrap().generation,
        trial.id
    );
    assert!(store.pending(session.id).await.unwrap().is_empty());
    provider.embedding_dimensions.store(64, Ordering::SeqCst);
    assert!(
        memories
            .recall("next sync", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .is_none()
    );

    let broken_id = registry
        .deploy("fixture", b"broken".to_vec())
        .await
        .unwrap()
        .generation
        .id;
    provider
        .steps
        .lock()
        .unwrap()
        .extend([reply("retried"), reply("notice received")]);
    kernel
        .submit(session.id, EventId::new(), "recover".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let starts: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted {
                attempt,
                round,
                plan,
                provider: CodeRef::Generation { id },
                ..
            } => Some((attempt, round, plan, id)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[0].1, starts[1].1);
    assert_eq!(starts[0].2, starts[1].2);
    assert_eq!(starts[0].3, broken_id);
    assert_eq!(starts[1].3, trial.id);
    assert_ne!(starts[1].1, starts[2].1);
    let request = broken.requests.lock().unwrap()[0].clone();
    assert_eq!(request, provider.requests.lock().unwrap()[2]);
    for start in &starts[..2] {
        let inspected = kernel.inspect(session.id, Some(start.0)).await.unwrap();
        assert_eq!(inspected.request, request);
        assert_eq!(inspected.settings, settings_profile.reply.settings);
    }
    let notices: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::EventConsumed { event }
                if matches!(event.body, EventBody::GenerationRolledBack(_)) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(
        provider.requests.lock().unwrap()[3]
            .messages
            .contains(&notices[0].canonical_message())
    );
    assert_eq!(
        registry.status().await[0]
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Failed
    );
    kernel.shutdown().await.unwrap();
    drop(kernel);
    drop(memories);
    drop(registry);
    let registry = open(store, runtime, FACTORY_BYTES, vec![]).await.unwrap();
    let status = registry.status().await.remove(0);
    for (id, code) in [
        (embedding_id, code::PLUGIN_CONTRACT),
        (broken_id, code::PLUGIN_TRAP),
    ] {
        let generation = status
            .generations
            .iter()
            .find(|record| record.id == id)
            .unwrap();
        assert_eq!(generation.failure, Some(fault(code)));
    }
}
