use crate::{
    bindings::{
        BasePre, CompletionPluginPre, DecisionPluginPre, EmbeddingPluginPre, enco::plugin::types,
    },
    convert,
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
use wasmtime_wasi::WasiCtx;

/// Compiled exports with no captured endpoint or credentials.
pub(super) struct WasmPlugin {
    pub lifecycle: BasePre<HostState>,
    pub completion: Option<CompletionPluginPre<HostState>>,
    pub embedding: Option<EmbeddingPluginPre<HostState>>,
    pub decision: Option<DecisionPluginPre<HostState>>,
    pub engine: Arc<WasmEngine>,
    pub http: reqwest::Client,
    pub name: String,
}

impl WasmPlugin {
    /// Every logical call owns a fresh Store and shares the same limits and failure boundary.
    async fn invoke<T>(
        &self,
        call: impl AsyncFnOnce(Store<HostState>) -> wasmtime::Result<Result<T, types::Failure>>,
    ) -> Result<T, Failure> {
        let state = HostState {
            wasi: WasiCtx::builder().inherit_stderr().build(),
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
        tokio::time::timeout(limits::PLUGIN_CALL_TIMEOUT, call(store))
            .await
            .map_err(|_| Failure {
                code: code::TIMEOUT.into(),
                message: "plugin invocation timed out".into(),
                retryable: true,
            })?
            .map_err(|error| trap(format!("plugin call failed: {error}")))?
            .map_err(convert::failure)
    }

    pub async fn describe(&self, config: &serde_json::Value) -> Result<String, Failure> {
        let config = config.to_string();
        self.invoke(async |mut store| {
            let plugin = self.lifecycle.instantiate_async(&mut store).await?;
            Ok(plugin
                .enco_plugin_lifecycle()
                .call_describe(&mut store, &config)
                .await?
                .map(|description| description.summary))
        })
        .await
    }
}

fn settings(settings: &ProviderSettings, api_key: Option<&str>) -> types::Settings {
    types::Settings {
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        api_key: api_key.map(str::to_owned),
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
        self.invoke(async |mut store| {
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
        api_key: Option<&str>,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        let pre = self
            .completion
            .as_ref()
            .ok_or_else(|| trap("completion export is missing"))?;
        let request = convert::request(request);
        let settings = settings(params, api_key);
        let result = self
            .invoke(async move |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                store
                    .run_concurrent(async move |accessor| {
                        plugin
                            .enco_plugin_completion()
                            .call_complete(accessor, settings, request)
                            .await
                    })
                    .await?
            })
            .await?;
        convert::completion(result)
    }
}

#[async_trait]
impl Embedding for WasmPlugin {
    async fn embed(
        &self,
        params: &ProviderSettings,
        api_key: Option<&str>,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        let pre = self
            .embedding
            .as_ref()
            .ok_or_else(|| trap("embedding export is missing"))?;
        let count = inputs.len();
        let settings = settings(params, api_key);
        let vectors = self
            .invoke(async move |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                store
                    .run_concurrent(async move |accessor| {
                        plugin
                            .enco_plugin_embedding()
                            .call_embed(accessor, settings, inputs)
                            .await
                    })
                    .await?
            })
            .await?;
        convert::embeddings(vectors, count)
    }
}

#[async_trait]
impl Decision for WasmPlugin {
    async fn decide(
        &self,
        params: &ProviderSettings,
        api_key: Option<&str>,
        state: String,
        questions: Vec<Question>,
    ) -> Result<Vec<Answer>, Failure> {
        let pre = self
            .decision
            .as_ref()
            .ok_or_else(|| trap("decision export is missing"))?;
        let wire_questions = questions.iter().map(convert::question).collect();
        let settings = settings(params, api_key);
        let answers = self
            .invoke(async move |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                store
                    .run_concurrent(async move |accessor| {
                        plugin
                            .enco_plugin_decision()
                            .call_decide(accessor, settings, state, wire_questions)
                            .await
                    })
                    .await?
            })
            .await?;
        convert::answers(answers, &questions)
    }
}
