//! One loaded plugin and the calls into it.
//!
//! Each call instantiates the component in a fresh store, under the same memory limit and timeout,
//! so nothing a guest keeps in memory survives to the next call.
use crate::{
    bindings::{
        BasePre, CompletionPluginPre, DecisionPluginPre, EmbeddingPluginPre, enco::plugin::types,
        lifecycle,
    },
    call_world::CallWorld,
    convert,
    credentials::{self, Credential},
    engine::WasmEngine,
    host_imports::HostState,
    limits,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{
    Answer, Completion, Decision, Embedding, Lifecycle, Provider, ProviderRequest, Question,
};
use std::sync::Arc;
use wasmtime::{Store, StoreLimitsBuilder, component::ResourceTable};

/// Compiled exports with no captured endpoint or credential value.
pub(super) struct WasmPlugin {
    pub lifecycle: BasePre<HostState>,
    pub completion: Option<CompletionPluginPre<HostState>>,
    pub embedding: Option<EmbeddingPluginPre<HostState>>,
    pub decision: Option<DecisionPluginPre<HostState>>,
    pub settings: PluginSettings,
    pub engine: Arc<WasmEngine>,
    pub http: reqwest::Client,
    pub name: String,
    pub credentials: Vec<Credential>,
    pub channel_origins: Vec<crate::origins::Origin>,
}

impl WasmPlugin {
    /// Every logical call owns a fresh Store and shares the same limits and failure boundary.
    pub(super) async fn invoke<T>(
        &self,
        world: CallWorld,
        cancel: tokio_util::sync::CancellationToken,
        call: impl AsyncFnOnce(Store<HostState>) -> wasmtime::Result<Result<T, types::Failure>>,
    ) -> Result<T, Failure> {
        let (network, config, wasi) = world.configure(&self.settings.config)?;
        let state = HostState {
            network,
            config,
            wasi,
            table: ResourceTable::new(),
            http: self.http.clone(),
            name: self.name.clone(),
            limits: StoreLimitsBuilder::new()
                .memory_size(limits::WASM_MEMORY_LIMIT)
                .build(),
        };
        let mut store = Store::new(&self.engine.engine, state);
        store.limiter(|state| &mut state.limits);
        store.epoch_deadline_async_yield_and_update(1);
        let invoke = tokio::time::timeout(limits::PLUGIN_CALL_TIMEOUT, call(store));
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Failure {
                code: code::CANCELLED.into(),
                message: "plugin invocation cancelled".into(),
                retryable: false,
            }),
            result = invoke => result,
        };
        result
            .map_err(|_| Failure {
                code: code::TIMEOUT.into(),
                message: "plugin invocation timed out".into(),
                retryable: true,
            })?
            .map_err(|error| trap(format!("plugin call failed: {error}")))?
            .map_err(convert::failure)
    }

    pub async fn describe(
        &self,
        config: &serde_json::Value,
    ) -> Result<lifecycle::Description, Failure> {
        let config = config.to_string();
        self.invoke(
            CallWorld::Declaration,
            Default::default(),
            async |mut store| {
                let plugin = self.lifecycle.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_lifecycle()
                    .call_describe(&mut store, &config)
                    .await
            },
        )
        .await
    }
}

fn settings(settings: &ProviderSettings) -> types::Settings {
    types::Settings {
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        options: settings.options.to_string(),
    }
}

fn trap(message: impl Into<String>) -> Failure {
    Failure {
        code: code::PLUGIN_TRAP.into(),
        message: message.into(),
        retryable: false,
    }
}

#[async_trait]
impl Lifecycle for WasmPlugin {
    async fn probe(&self) -> Result<(), Failure> {
        self.invoke(CallWorld::Probe, Default::default(), async |mut store| {
            let plugin = self.lifecycle.instantiate_async(&mut store).await?;
            store
                .run_concurrent(async move |accessor| {
                    plugin.enco_plugin_lifecycle().call_probe(accessor).await
                })
                .await?
        })
        .await
    }
}

#[async_trait]
impl Provider for WasmPlugin {
    async fn complete(
        &self,
        params: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        let pre = self
            .completion
            .as_ref()
            .ok_or_else(|| trap("completion export is missing"))?;
        let request = convert::request(request);
        let settings = settings(params);
        let credentials = credentials::resolve(&self.credentials, &self.settings, Some(params))?;
        let result = self
            .invoke(
                CallWorld::Regular(credentials),
                Default::default(),
                async move |mut store| {
                    let plugin = pre.instantiate_async(&mut store).await?;
                    store
                        .run_concurrent(async move |accessor| {
                            plugin
                                .enco_plugin_completion()
                                .call_complete(accessor, settings, request)
                                .await
                        })
                        .await?
                },
            )
            .await?;
        convert::completion(result)
    }
}

#[async_trait]
impl Embedding for WasmPlugin {
    async fn embed(
        &self,
        params: &ProviderSettings,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        let pre = self
            .embedding
            .as_ref()
            .ok_or_else(|| trap("embedding export is missing"))?;
        let count = inputs.len();
        let settings = settings(params);
        let credentials = credentials::resolve(&self.credentials, &self.settings, Some(params))?;
        let vectors = self
            .invoke(
                CallWorld::Regular(credentials),
                Default::default(),
                async move |mut store| {
                    let plugin = pre.instantiate_async(&mut store).await?;
                    store
                        .run_concurrent(async move |accessor| {
                            plugin
                                .enco_plugin_embedding()
                                .call_embed(accessor, settings, inputs)
                                .await
                        })
                        .await?
                },
            )
            .await?;
        convert::embeddings(vectors, count)
    }
}

#[async_trait]
impl Decision for WasmPlugin {
    async fn decide(
        &self,
        params: &ProviderSettings,
        state: String,
        questions: Vec<Question>,
    ) -> Result<Vec<Answer>, Failure> {
        let pre = self
            .decision
            .as_ref()
            .ok_or_else(|| trap("decision export is missing"))?;
        let wire_questions = questions.iter().map(convert::question).collect();
        let settings = settings(params);
        let credentials = credentials::resolve(&self.credentials, &self.settings, Some(params))?;
        let answers = self
            .invoke(
                CallWorld::Regular(credentials),
                Default::default(),
                async move |mut store| {
                    let plugin = pre.instantiate_async(&mut store).await?;
                    store
                        .run_concurrent(async move |accessor| {
                            plugin
                                .enco_plugin_decision()
                                .call_decide(accessor, settings, state, wire_questions)
                                .await
                        })
                        .await?
                },
            )
            .await?;
        convert::answers(answers, &questions)
    }
}
