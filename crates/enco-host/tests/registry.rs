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
        registry.exports().completion("fixture").unwrap().generation,
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
    assert_eq!(status.active.unwrap().id, factory);
    assert!(
        status
            .generations
            .iter()
            .filter(|record| record.id != factory)
            .all(|record| record.status == GenerationStatus::Failed)
    );

    let only = registry
        .deploy("custom", b"latest".to_vec())
        .await
        .unwrap()
        .generation;
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
        registry.exports().completion("custom").unwrap().generation,
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
        registry.exports().completion("custom").err().unwrap().code,
        code::PLUGIN_UNAVAILABLE
    );
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
    registry.deploy("fixture", b"two".to_vec()).await.unwrap();
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
    assert_eq!(previous.id, three.id.min(four.id));
    assert!(
        registry.status().await[0]
            .generations
            .iter()
            .all(|record| record.status == GenerationStatus::Healthy)
    );
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
    new.embedding_failure.store(true, Ordering::SeqCst);
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(old.clone()));
    runtime.insert(b"new", loaded(new.clone()));
    let registry = open(store.clone(), runtime, FACTORY_BYTES, vec![])
        .await
        .unwrap();
    let factory = registry.exports().completion("fixture").unwrap().generation;
    let mut endpoint = endpoint();
    endpoint.settings.model = "per-call-model".into();
    let mut tools = native_tools(workspace.clone());
    tools.extend(plugin_tools(registry.clone(), workspace.clone()));
    let kernel = Kernel::start(
        KernelDeps {
            store,
            registry: registry.clone(),
            profile: Profile {
                reply: endpoint.clone(),
                compaction: endpoint.clone(),
            },
            composer: Arc::new(FactoryComposer::new(
                workspace,
                dir.path().join("AGENTS.md"),
            )),
            context: vec![],
            tools,
            lifeline: LIFELINE.iter().map(|name| (*name).into()).collect(),
            clock: Arc::new(SystemClock),
        },
        KernelConfig::new(
            Budget {
                context_tokens: 128000,
                max_output_tokens: 8192,
            },
            24,
        )
        .unwrap(),
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
            id: registry.exports().completion("fixture").unwrap().generation
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
