#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "shared test support: each test binary uses a subset of these helpers, which fail the test by panicking"
)]
use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;

/// A plugin whose admission probe has a fixed result.
pub struct ScriptedLifecycle(pub Result<(), Failure>);

#[async_trait]
impl Lifecycle for ScriptedLifecycle {
    async fn probe(&self) -> Result<(), Failure> {
        self.0.clone()
    }
}

pub struct ScriptedProvider {
    pub embedding_failure: Mutex<Option<Failure>>,
    pub embedding_dimensions: std::sync::atomic::AtomicUsize,
    pub steps: Mutex<VecDeque<Result<Completion, Failure>>>,
    pub requests: Mutex<Vec<ProviderRequest>>,
    pub settings: Mutex<Vec<ProviderSettings>>,
}

impl ScriptedProvider {
    pub fn new(steps: Vec<Result<Completion, Failure>>) -> Arc<Self> {
        Arc::new(Self {
            embedding_failure: Mutex::new(None),
            embedding_dimensions: std::sync::atomic::AtomicUsize::new(64),
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(vec![]),
            settings: Mutex::new(vec![]),
        })
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        self.requests.lock().unwrap().push(request);
        self.settings.lock().unwrap().push(settings.clone());
        self.steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra provider request")
    }
}

#[async_trait]
impl Embedding for ScriptedProvider {
    async fn embed(
        &self,
        _settings: &ProviderSettings,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        if let Some(failure) = self.embedding_failure.lock().unwrap().clone() {
            return Err(failure);
        }
        Ok(vectors(
            &inputs,
            self.embedding_dimensions
                .load(std::sync::atomic::Ordering::SeqCst),
        ))
    }
}

pub const FACTORY_BYTES: &[u8] = b"scripted-factory";
pub const FIXTURE_ID: &str = "01M3X4HYHSE2M3523YK35VX60W";

#[derive(Default)]
pub struct ScriptedRuntime {
    exports: Mutex<HashMap<ContentHash, Loaded>>,
}

impl ScriptedRuntime {
    pub fn insert(&self, bytes: &[u8], loaded: Loaded) {
        self.exports
            .lock()
            .unwrap()
            .insert(ContentHash::of(bytes), loaded);
    }
}

#[async_trait]
impl Runtime for ScriptedRuntime {
    async fn load(&self, bytes: &[u8], _config: &serde_json::Value) -> Result<Loaded, LoadError> {
        self.exports
            .lock()
            .unwrap()
            .get(&ContentHash::of(bytes))
            .cloned()
            .ok_or_else(|| LoadError("unknown fixture artifact".into()))
    }
}

pub fn store_paths(root: &Path) -> StorePaths {
    StorePaths {
        db: root.join("enco.db"),
        blobs: root.join("blobs"),
        artifacts: root.join("artifacts"),
        plugins_lock: root.join("plugins.lock"),
    }
}

/// Memory's table is required in production, so the fixture also configures an embedding.
pub fn configuration() -> serde_json::Value {
    serde_json::json!({
        "owner": {"timezone": "UTC"},
        "endpoint": {
            "reply": {"plugin": "fixture", "base_url": "https://fixture.invalid", "model": "fixture", "window_tokens": 128000, "max_output_tokens": 8192},
            "compaction": {"plugin": "fixture", "base_url": "https://fixture.invalid", "model": "fixture", "window_tokens": 128000, "max_output_tokens": 8192},
            "embed": {"plugin": "fixture", "base_url": "https://fixture.invalid", "model": "fixture", "dimensions": 64}
        },
        "profile": {"default": {"reply": ["reply"], "compaction": ["compaction"]}},
        "memory": {"embedding": "embed"}
    })
}

/// Exports of the factory fixture, which serves every configured use.
pub fn fixture(completion: Arc<dyn Provider>, embedding: Arc<dyn Embedding>) -> Loaded {
    Loaded {
        lifecycle: Arc::new(ScriptedLifecycle(Ok(()))),
        summary: "fixture".into(),
        config_schema: serde_json::json!({}),
        credential: None,
        completion: Some(completion),
        embedding: Some(embedding),
        decision: None,
    }
}

/// The registry as the composition root assembles it, over a scripted runtime.
pub fn registry_deps(
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    factory: &[u8],
    document: serde_json::Value,
) -> RegistryDeps {
    RegistryDeps {
        store,
        runtime,
        factory: vec![FactoryPlugin {
            name: "fixture".into(),
            id: FIXTURE_ID.parse().unwrap(),
            artifact: factory.to_vec(),
        }],
        owners: TABLE_OWNERS.into(),
        bootstrap: Ok(document),
        clock: Arc::new(SystemClock),
    }
}

pub async fn registry_configured(
    root: &Path,
    loaded: Loaded,
    document: serde_json::Value,
) -> (Arc<Registry>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded);
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

pub async fn open_memories(root: &Path, registry: Arc<Registry>) -> Arc<Memories> {
    Memories::open(
        MemoryPaths {
            db: root.join("memory.db"),
            index: root.join("memory-index"),
        },
        registry,
        Arc::new(SystemClock),
    )
    .await
    .unwrap()
}

/// The Kernel as the composition root assembles it over an already-built registry; memory
/// joins with its context source and tools when the test exercises it.
pub fn kernel_deps(
    root: &Path,
    store: Arc<SqliteStore>,
    registry: Arc<Registry>,
    memories: Option<Arc<Memories>>,
) -> KernelDeps {
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let instructions = root.join("AGENTS.md");
    let mut context: Vec<Arc<dyn ContextSource>> = vec![Arc::new(InstructionsContextSource::new(
        instructions.clone(),
    ))];
    let mut tools = native_tools(workspace.clone());
    if let Some(memories) = memories {
        context.push(Arc::new(MemoryContextSource::new(memories.clone())));
        tools.extend(memory_tools(memories));
    }
    tools.extend(plugin_tools(registry.clone(), workspace.clone()));
    tools.extend(config::tools(root.join("config.toml"), registry.clone()));
    KernelDeps {
        store,
        registry,
        composer: Arc::new(FactoryComposer::new(workspace, instructions)),
        context,
        tools,
        lifeline: LIFELINE.iter().map(|name| (*name).into()).collect(),
        clock: Arc::new(SystemClock),
    }
}

pub fn reply(text: &str) -> Result<Completion, Failure> {
    Ok(Completion {
        message: Message::text(Role::Assistant, text),
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
            cached_input_tokens: None,
        },
        stop: StopReason::EndTurn,
    })
}

pub fn call(name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: CallId::new(),
        provider_id: format!("provider-{}", CallId::new()),
        name: name.into(),
        arguments: args.into(),
    }
}

pub fn calls(calls: Vec<ToolCall>) -> Result<Completion, Failure> {
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts: calls.into_iter().map(Part::ToolCall).collect(),
        },
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
            cached_input_tokens: None,
        },
        stop: StopReason::ToolCalls,
    })
}

pub async fn kernel(root: &Path, provider: Arc<dyn Provider>) -> (Kernel, Arc<SqliteStore>) {
    kernel_with(root, provider, Tz::UTC, |_| {}).await
}

pub async fn kernel_with(
    root: &Path,
    provider: Arc<dyn Provider>,
    timezone: Tz,
    configure: impl FnOnce(&mut KernelDeps),
) -> (Kernel, Arc<SqliteStore>) {
    let mut document = configuration();
    document["owner"]["timezone"] = serde_json::json!(timezone);
    kernel_configured(root, provider, document, configure).await
}

pub async fn kernel_configured(
    root: &Path,
    provider: Arc<dyn Provider>,
    document: serde_json::Value,
    configure: impl FnOnce(&mut KernelDeps),
) -> (Kernel, Arc<SqliteStore>) {
    // Without memory nothing recalls; the export only satisfies memory's declared use.
    let loaded = fixture(provider, ScriptedProvider::new(vec![]));
    let (registry, store) = registry_configured(root, loaded, document).await;
    let mut deps = kernel_deps(root, store.clone(), registry, None);
    configure(&mut deps);
    (Kernel::start(deps).await.unwrap(), store)
}

/// A node whose Kernel and Memories follow one registry; the provider serves both uses.
pub async fn kernel_with_memory(
    root: &Path,
    provider: Arc<ScriptedProvider>,
    document: serde_json::Value,
    configure: impl FnOnce(&mut KernelDeps),
) -> (Kernel, Arc<Memories>) {
    let loaded = fixture(provider.clone(), provider);
    let (registry, store) = registry_configured(root, loaded, document).await;
    let memories = open_memories(root, registry.clone()).await;
    let mut deps = kernel_deps(root, store, registry, Some(memories.clone()));
    configure(&mut deps);
    (Kernel::start(deps).await.unwrap(), memories)
}

pub async fn finish(rx: &mut broadcast::Receiver<Entry>) -> Vec<Entry> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut entries = vec![];
        loop {
            let entry = rx.recv().await.unwrap();
            let ended = matches!(entry.body, EntryBody::RunEnded { .. });
            entries.push(entry);
            if ended {
                return entries;
            }
        }
    })
    .await
    .expect("Run did not end")
}

pub fn vectors(inputs: &[String], dimensions: usize) -> Vec<Vec<f32>> {
    inputs
        .iter()
        .map(|text| {
            let mut vector = vec![0.0f32; dimensions];
            let chars: Vec<_> = text.chars().collect();
            for pair in chars.windows(2) {
                vector[((pair[0] as usize).wrapping_mul(31) + pair[1] as usize) % dimensions] +=
                    1.0;
            }
            let norm = vector.iter().map(|n| n * n).sum::<f32>().sqrt();
            if norm > 0.0 {
                for value in &mut vector {
                    *value /= norm;
                }
            } else {
                vector[0] = 1.0;
            }
            vector
        })
        .collect()
}

pub fn is_reminder(entry: &Entry, id: ScheduleId) -> bool {
    let EntryBody::EventConsumed { event } = &entry.body else {
        return false;
    };
    matches!(event.body, EventBody::Reminder { schedule, .. } if schedule == id)
}
