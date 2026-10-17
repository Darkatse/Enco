#![allow(
    dead_code,
    unused_imports,
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

mod development;
pub use development::*;

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
    async fn load(&self, bytes: &[u8], _config: &PluginSettings) -> Result<Loaded, LoadError> {
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

pub fn fixture_endpoint(plugin: &str) -> Endpoint {
    let mut endpoint = KernelSettings::from_document(&configuration())
        .unwrap()
        .profiles["default"]
        .reply
        .first()
        .endpoint
        .clone();
    endpoint.plugin = plugin.into();
    endpoint
}

/// Exports of the factory fixture, which serves every configured use.
pub fn fixture(completion: Arc<dyn Provider>, embedding: Arc<dyn Embedding>) -> Loaded {
    Loaded {
        lifecycle: Arc::new(ScriptedLifecycle(Ok(()))),
        summary: "fixture".into(),
        config_schema: serde_json::json!({}),
        credentials: Vec::new(),
        unavailable: None,
        tools: None,
        completion: Some(completion),
        embedding: Some(embedding),
        decision: None,
    }
}

pub fn registry_deps(
    store: Arc<SqliteStore>,
    runtime: Arc<dyn Runtime>,
    factory: &[u8],
) -> RegistryDeps {
    RegistryDeps {
        host_tool_names: native_tools(Default::default())
            .iter()
            .map(|tool| tool.spec())
            .chain([self_status_spec()])
            .chain(builtin_tool_specs())
            .map(|spec| spec.name)
            .collect(),
        store,
        runtime,
        factory: vec![FactoryPlugin {
            name: "fixture".into(),
            id: FIXTURE_ID.parse().unwrap(),
            artifact: factory.to_vec(),
        }],
        owners: TABLE_OWNERS.into(),
        clock: Arc::new(SystemClock),
    }
}

pub fn factory_runtime(loaded: Loaded) -> Arc<ScriptedRuntime> {
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded);
    runtime
}

pub async fn registry_configured(
    root: &Path,
    loaded: Loaded,
    document: serde_json::Value,
    extra_tools: Vec<ToolSpec>,
) -> (Arc<Registry>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let mut deps = registry_deps(store.clone(), factory_runtime(loaded), FACTORY_BYTES);
    deps.host_tool_names
        .extend(extra_tools.into_iter().map(|spec| spec.name));
    let registry = open_registry(deps, configuration_request(document))
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

/// The lifeline is the host's own, so a kernel without its home's registration tools fails to start.
pub fn kernel_deps(
    root: &Path,
    store: Arc<SqliteStore>,
    registry: Arc<Registry>,
    memories: Option<Arc<Memories>>,
    development: Option<Arc<enco_host::development::Development>>,
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
    if let Some(development) = development {
        tools.extend(development_tools(development, registry.clone()));
    }
    tools.push(Arc::new(TestStatus(registry.clone())));
    KernelDeps {
        store,
        registry,
        composer: Arc::new(FactoryComposer::new(workspace, instructions)),
        context,
        tools,
        lifeline: LIFELINE.map(String::from).into(),
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
    kernel_configured(root, provider, document, vec![], configure).await
}

pub async fn kernel_configured(
    root: &Path,
    provider: Arc<dyn Provider>,
    document: serde_json::Value,
    tools: Vec<Arc<dyn Tool>>,
    configure: impl FnOnce(&mut KernelDeps),
) -> (Kernel, Arc<SqliteStore>) {
    // Without memory nothing recalls; the export only satisfies memory's declared use.
    let runtime = factory_runtime(fixture(provider, ScriptedProvider::new(vec![])));
    let extra = tools.iter().map(|tool| tool.spec()).collect();
    let (development, registry, store) = scripted_home(root, runtime, document, extra).await;
    let mut deps = kernel_deps(root, store.clone(), registry, None, Some(development));
    deps.tools.extend(tools);
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
    let runtime = factory_runtime(fixture(provider.clone(), provider));
    let (development, registry, store) =
        scripted_home(root, runtime, document, memory_tool_specs()).await;
    let memories = open_memories(root, registry.clone()).await;
    let mut deps = kernel_deps(
        root,
        store,
        registry,
        Some(memories.clone()),
        Some(development),
    );
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

/// Supply the initial factory candidates at the host boundary; reopening only restores.
pub async fn open_registry(
    deps: RegistryDeps,
    mut request: RegistrationRequest,
) -> Result<Arc<Registry>, RegistryError> {
    Registry::open(deps, |registered| async move {
        for (name, plugin) in registered.plugins {
            if plugin.factory.is_some() {
                request.plugins.entry(name).or_insert(PluginChange::Factory);
            }
        }
        Ok::<_, RegistryError>(request)
    })
    .await
}

/// Tests offer candidates against the same accepted baseline a host would capture.
pub async fn submit(
    registry: &Registry,
    mut request: RegistrationRequest,
) -> Result<Registered, RegistryError> {
    request.base = registry.registered_intent().await.base.map(|base| base.id);
    registry.register(request).await
}

pub fn configuration_request(document: serde_json::Value) -> RegistrationRequest {
    RegistrationRequest {
        revision: format!("fixture-config-{}", EventId::new()),
        configuration: Some(document),
        base: None,
        plugins: Default::default(),
    }
}

pub fn artifact_request(name: &str, bytes: Vec<u8>) -> RegistrationRequest {
    RegistrationRequest {
        revision: format!("fixture-artifact-{}", EventId::new()),
        plugins: [(name.into(), PluginChange::Artifact(bytes))].into(),
        base: None,
        configuration: None,
    }
}

pub fn source_request(name: &str, bytes: &[u8]) -> RegistrationRequest {
    RegistrationRequest {
        revision: format!("fixture-source-{}", EventId::new()),
        plugins: [(
            name.into(),
            PluginChange::Code(ArtifactSource::Bytes(bytes.into())),
        )]
        .into(),
        base: None,
        configuration: None,
    }
}

pub fn credential(required: bool) -> Credential {
    Credential {
        name: "service".into(),
        origin: None,
        required,
        header: "authorization".into(),
        prefix: "Bearer ".into(),
    }
}

/// Kernel fixtures expose registry facts; the complete application status is covered by CLI tests.
struct TestStatus(Arc<Registry>);

#[async_trait]
impl Tool for TestStatus {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "test-status".into(),
            version: "1".into(),
        }
    }
    fn spec(&self) -> ToolSpec {
        self_status_spec()
    }
    async fn call(
        &self,
        _ctx: CallContext,
        _args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        Outcome::Ok {
            value: serde_json::json!(self.0.status().await.unwrap()),
        }
    }
}
