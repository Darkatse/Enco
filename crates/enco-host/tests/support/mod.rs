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

pub struct Probe(pub Result<(), Failure>);

#[async_trait]
impl Lifecycle for Probe {
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
        _api_key: Option<&str>,
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
        _api_key: Option<&str>,
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

pub fn settings() -> ProviderSettings {
    ProviderSettings {
        base_url: "https://fixture.invalid".into(),
        model: "fixture".into(),
        api_key_env: None,
        options: serde_json::json!({}),
    }
}

pub fn endpoint() -> Endpoint {
    Endpoint {
        plugin: "fixture".into(),
        settings: settings(),
        api_key: None,
        budget: Budget {
            context_tokens: 128000,
            max_output_tokens: 8192,
        },
    }
}

pub fn profile() -> Profile {
    Profile {
        reply: endpoint(),
        compaction: endpoint(),
        requires_lifeline: true,
    }
}

pub fn embedding_endpoint(model: &str, dimensions: usize) -> EmbeddingEndpoint {
    EmbeddingEndpoint {
        plugin: "fixture".into(),
        settings: ProviderSettings {
            model: model.into(),
            ..settings()
        },
        api_key: None,
        dimensions,
    }
}

pub async fn registry(root: &Path, loaded: Loaded) -> (Arc<Registry>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded);
    let registry = Registry::open(RegistryDeps {
        store: store.clone(),
        runtime,
        factory: vec![FactoryPlugin {
            name: "fixture".into(),
            id: FIXTURE_ID.parse().unwrap(),
            artifact: FACTORY_BYTES.to_vec(),
        }],
        wiring: vec![],
        clock: Arc::new(SystemClock),
    })
    .await
    .unwrap();
    (registry, store)
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
    let (registry, store) = registry(
        root,
        Loaded {
            lifecycle: Arc::new(Probe(Ok(()))),
            summary: "scripted".into(),
            completion: Some(provider),
            embedding: None,
            decision: None,
        },
    )
    .await;
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut deps = KernelDeps {
        store: store.clone(),
        profiles: [("default".into(), profile())].into(),
        registry: registry.clone(),
        composer: Arc::new(FactoryComposer::new(
            workspace.clone(),
            root.join("AGENTS.md"),
        )),
        context: vec![Arc::new(InstructionsContextSource::new(
            root.join("AGENTS.md"),
        ))],
        tools: native_tools(workspace.clone()),
        lifeline: LIFELINE.iter().map(|s| s.to_string()).collect(),
        clock: Arc::new(SystemClock),
    };
    deps.tools.extend(plugin_tools(registry, workspace));
    configure(&mut deps);
    let config = KernelConfig::new(24, timezone).unwrap();
    (Kernel::start(deps, config).await.unwrap(), store)
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
