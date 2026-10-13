//! How Kernel attempts and memory calls follow the active generation and report on its health.
use super::{deploy_artifact, fault, loaded, open, promote};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// A Kernel and Memories following one registry, with a subscribed `main` Session.
struct Node {
    kernel: Kernel,
    memories: Arc<Memories>,
    session: SessionId,
    rx: broadcast::Receiver<Entry>,
}

impl Node {
    async fn start(root: &Path, store: Arc<SqliteStore>, registry: Arc<Registry>) -> Self {
        let kernel = Kernel::start(kernel_deps(root, store, registry.clone(), None))
            .await
            .unwrap();
        let memories = open_memories(root, registry).await;
        let session = kernel.open_session("main").await.unwrap().id;
        let rx = kernel.subscribe(session).unwrap();
        Self {
            kernel,
            memories,
            session,
            rx,
        }
    }

    async fn run(&mut self, text: &str) -> Vec<Entry> {
        self.kernel
            .submit(self.session, EventId::new(), text.into())
            .await
            .unwrap();
        finish(&mut self.rx).await
    }
}

/// Factory code whose first reply deploys `new.wasm` through the plugin tool; the new code's
/// embedding fails.
struct ToolDeployment {
    node: Node,
    old: Arc<ScriptedProvider>,
    new: Arc<ScriptedProvider>,
    factory: GenerationId,
    settings: ProviderSettings,
}

impl ToolDeployment {
    /// Returns the node after the deploying Run, with that Run's entries.
    async fn replace_provider(root: &Path) -> (Self, Vec<Entry>) {
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("new.wasm"), b"new").unwrap();
        let old = ScriptedProvider::new(vec![calls(vec![call(
            "plugin_deploy",
            r#"{"name":"fixture","path":"new.wasm"}"#,
        )])]);
        let new = ScriptedProvider::new(vec![reply("new code")]);
        *new.embedding_failure.lock().unwrap() = Some(fault(code::PROVIDER_NETWORK));
        let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
        let runtime = Arc::new(ScriptedRuntime::default());
        runtime.insert(FACTORY_BYTES, loaded(old.clone()));
        runtime.insert(b"new", loaded(new.clone()));
        let mut document = configuration();
        for name in ["reply", "compaction"] {
            document["endpoint"][name]["model"] = "per-call-model".into();
        }
        let registry = Registry::open(registry_deps(
            store.clone(),
            runtime,
            FACTORY_BYTES,
            document,
        ))
        .await
        .unwrap();
        let factory = registry
            .view()
            .exports
            .completion("fixture", false)
            .unwrap()
            .generation;
        let settings = registry.view().config.kernel.profiles["default"]
            .reply
            .endpoint
            .settings
            .clone();
        let mut node = Node::start(root, store, registry).await;
        let entries = node.run("replace your provider").await;
        let deployment = Self {
            node,
            old,
            new,
            factory,
            settings,
        };
        (deployment, entries)
    }
}

#[tokio::test]
async fn deployment_tool_moves_the_next_attempt_and_memory_call_to_the_new_code() {
    let dir = tempfile::tempdir().unwrap();
    let (deployment, entries) = ToolDeployment::replace_provider(dir.path()).await;
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
    assert_eq!(starts[0].0, &deployment.factory);
    assert_eq!(
        starts[1].0,
        &deployment
            .node
            .kernel
            .registry()
            .view()
            .exports
            .completion("fixture", false)
            .unwrap()
            .generation
    );
    assert!(
        starts
            .iter()
            .all(|(_, settings)| **settings == deployment.settings)
    );
    assert_eq!(
        deployment.new.settings.lock().unwrap().as_slice(),
        std::slice::from_ref(&deployment.settings)
    );
    assert!(
        deployment
            .node
            .memories
            .recall("after", 1, &CancellationToken::new())
            .await
            .unwrap()
            .lexical_only
            .is_some()
    );
    deployment.node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn deployment_tool_restores_registered_code_as_a_new_generation() {
    let dir = tempfile::tempdir().unwrap();
    let (mut deployment, _) = ToolDeployment::replace_provider(dir.path()).await;
    deployment
        .new
        .steps
        .lock()
        .unwrap()
        .push_back(calls(vec![call(
            "plugin_deploy",
            &serde_json::json!({"name": "fixture", "generation": deployment.factory}).to_string(),
        )]));
    deployment
        .old
        .steps
        .lock()
        .unwrap()
        .push_back(reply("restored"));
    let entries = deployment.node.run("restore your provider").await;
    let returned = entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Ok,
                content,
                ..
            } => Some(
                serde_json::from_value::<GenerationRecord>(
                    serde_json::from_str::<serde_json::Value>(content).unwrap()["generation"]
                        .clone(),
                )
                .unwrap(),
            ),
            _ => None,
        })
        .unwrap();
    assert!(returned.id > deployment.factory);
    assert_eq!(returned.artifact, ContentHash::of(FACTORY_BYTES));
    assert!(
        deployment
            .node
            .memories
            .recall("restored", 1, &CancellationToken::new())
            .await
            .unwrap()
            .lexical_only
            .is_none()
    );
    deployment.node.kernel.shutdown().await.unwrap();
}

/// A node whose `fixture` plugin runs the `working` trial over factory code, with the `broken`
/// artifact ready to deploy; providers start unscripted.
struct Callers {
    node: Node,
    store: Arc<SqliteStore>,
    runtime: Arc<ScriptedRuntime>,
    registry: Arc<Registry>,
    factory: Arc<ScriptedProvider>,
    provider: Arc<ScriptedProvider>,
    broken: Arc<ScriptedProvider>,
    factory_id: GenerationId,
    trial: GenerationId,
    settings: ProviderSettings,
}

impl Callers {
    async fn start(root: &Path) -> Self {
        let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
        let runtime = Arc::new(ScriptedRuntime::default());
        let factory = ScriptedProvider::new(vec![]);
        let provider = ScriptedProvider::new(vec![]);
        let broken = ScriptedProvider::new(vec![]);
        runtime.insert(FACTORY_BYTES, loaded(factory.clone()));
        runtime.insert(b"working", loaded(provider.clone()));
        runtime.insert(b"broken", loaded(broken.clone()));
        let mut document = configuration();
        document["endpoint"]["reply"]["model"] = "chosen-model".into();
        let registry = Registry::open(registry_deps(
            store.clone(),
            runtime.clone(),
            FACTORY_BYTES,
            document,
        ))
        .await
        .unwrap();
        let factory_id = registry
            .view()
            .exports
            .completion("fixture", true)
            .unwrap()
            .generation;
        let trial = deploy_artifact(&registry, "fixture", b"working").await.id;
        let settings = registry.view().config.kernel.profiles["default"]
            .reply
            .endpoint
            .settings
            .clone();
        let node = Node::start(root, store.clone(), registry.clone()).await;
        Self {
            node,
            store,
            runtime,
            registry,
            factory,
            provider,
            broken,
            factory_id,
            trial,
            settings,
        }
    }

    async fn active_status(&self) -> GenerationStatus {
        self.registry.status().await.plugins[0]
            .active
            .as_ref()
            .unwrap()
            .status
    }

    /// Shuts the node down and returns the generations a reopened registry restores.
    async fn restart(self) -> Vec<GenerationRecord> {
        let Self {
            node,
            store,
            runtime,
            registry,
            ..
        } = self;
        node.kernel.shutdown().await.unwrap();
        drop((node, registry));
        let registry = open(&store, runtime).await;
        registry.status().await.plugins.remove(0).generations
    }
}

#[tokio::test]
async fn attempts_use_the_factory_in_safe_mode_and_the_trial_otherwise_with_profile_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut callers = Callers::start(dir.path()).await;
    callers
        .factory
        .steps
        .lock()
        .unwrap()
        .push_back(reply("safe mode"));
    callers
        .provider
        .steps
        .lock()
        .unwrap()
        .push_back(reply("active"));
    for (safe, expected) in [(true, callers.factory_id), (false, callers.trial)] {
        callers.node.kernel.set_safe_mode(safe).await.unwrap();
        let entries = callers.node.run("reply").await;
        let attempt = entries
            .iter()
            .find_map(|entry| match entry.body {
                EntryBody::AttemptStarted {
                    attempt, provider, ..
                } => {
                    assert_eq!(provider, expected);
                    Some(attempt)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            callers
                .node
                .kernel
                .inspect(callers.node.session, Some(attempt))
                .await
                .unwrap()
                .settings,
            callers.settings
        );
    }
    callers.node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn kernel_attempts_and_memory_calls_count_toward_one_trial() {
    let dir = tempfile::tempdir().unwrap();
    let mut callers = Callers::start(dir.path()).await;
    callers
        .provider
        .steps
        .lock()
        .unwrap()
        .extend([reply("active"), reply("healthy")]);
    callers.node.run("reply").await;
    // An index configuration mismatch follows a valid plugin result and still counts.
    callers
        .provider
        .embedding_dimensions
        .store(32, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    for _ in 0..TRIAL_CALLS - 2 {
        assert!(
            callers
                .node
                .memories
                .recall("memory", 1, &cancel)
                .await
                .unwrap()
                .lexical_only
                .is_some()
        );
    }
    assert_eq!(callers.active_status().await, GenerationStatus::Trial);
    callers.node.run("one more success").await;
    assert_eq!(callers.active_status().await, GenerationStatus::Healthy);
    callers.node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_memory_call_rolls_back_without_a_session_notice() {
    let dir = tempfile::tempdir().unwrap();
    let callers = Callers::start(dir.path()).await;
    promote(&callers.registry, callers.trial).await;
    *callers.broken.embedding_failure.lock().unwrap() = Some(fault(code::PLUGIN_CONTRACT));
    let embedding_id = deploy_artifact(&callers.registry, "fixture", b"broken")
        .await
        .id;
    let cancel = CancellationToken::new();
    assert_eq!(
        callers
            .node
            .memories
            .recall("failed embedding", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .unwrap()
            .code,
        code::PLUGIN_CONTRACT
    );
    assert_eq!(
        callers
            .registry
            .view()
            .exports
            .embedding("fixture")
            .unwrap()
            .generation,
        callers.trial
    );
    assert!(
        callers
            .store
            .pending(callers.node.session)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        callers
            .node
            .memories
            .recall("next sync", 1, &cancel)
            .await
            .unwrap()
            .lexical_only
            .is_none()
    );
    let generations = callers.restart().await;
    let generation = generations
        .iter()
        .find(|record| record.id == embedding_id)
        .unwrap();
    assert_eq!(generation.failure, Some(fault(code::PLUGIN_CONTRACT)));
}

#[tokio::test]
async fn a_failed_attempt_retries_its_recorded_request_on_the_rollback_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut callers = Callers::start(dir.path()).await;
    promote(&callers.registry, callers.trial).await;
    callers
        .broken
        .steps
        .lock()
        .unwrap()
        .push_back(Err(fault(code::PLUGIN_TRAP)));
    let broken_id = deploy_artifact(&callers.registry, "fixture", b"broken")
        .await
        .id;
    callers
        .provider
        .steps
        .lock()
        .unwrap()
        .extend([reply("retried"), reply("notice received")]);
    let entries = callers.node.run("recover").await;
    let starts: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::AttemptStarted {
                attempt,
                round,
                plan,
                provider,
                ..
            } => Some((attempt, round, plan, provider)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[0].1, starts[1].1);
    assert_eq!(starts[0].2, starts[1].2);
    assert_eq!(starts[0].3, broken_id);
    assert_eq!(starts[1].3, callers.trial);
    assert_ne!(starts[1].1, starts[2].1);
    let request = callers.broken.requests.lock().unwrap()[0].clone();
    assert_eq!(request, callers.provider.requests.lock().unwrap()[0]);
    for start in &starts[..2] {
        let inspected = callers
            .node
            .kernel
            .inspect(callers.node.session, Some(start.0))
            .await
            .unwrap();
        assert_eq!(inspected.request, request);
        assert_eq!(inspected.settings, callers.settings);
    }
    let notices: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::EventConsumed { event }
                if matches!(event.body, EventBody::GenerationRolledBack { .. }) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(
        callers.provider.requests.lock().unwrap()[1]
            .messages
            .contains(&notices[0].canonical_message())
    );
    assert_eq!(
        callers.registry.status().await.plugins[0]
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Failed
    );
    let generations = callers.restart().await;
    let generation = generations
        .iter()
        .find(|record| record.id == broken_id)
        .unwrap();
    assert_eq!(generation.failure, Some(fault(code::PLUGIN_TRAP)));
}
