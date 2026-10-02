use crate::{
    bindings::{
        ProviderPlugin, ProviderPluginPre, enco::plugin::types,
        exports::enco::plugin::provider as wit,
    },
    convert,
    engine::WasmEngine,
    host_imports::HostState,
    limits,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{Completion, Embedding, Provider, ProviderRequest};
use std::sync::Arc;
use wasmtime::{Store, StoreLimitsBuilder, component::ResourceTable};
use wasmtime_wasi::WasiCtx;

/// Compiled code with no captured endpoint or credentials; each invocation owns a fresh Store.
pub(super) struct WasmPlugin {
    pub pre: ProviderPluginPre<HostState>,
    pub engine: Arc<WasmEngine>,
    pub http: reqwest::Client,
    pub name: String,
}

impl WasmPlugin {
    pub(super) async fn instance(&self) -> wasmtime::Result<(Store<HostState>, ProviderPlugin)> {
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
        let plugin = self.pre.instantiate_async(&mut store).await?;
        Ok((store, plugin))
    }

    async fn invoke<T>(
        &self,
        settings: &ProviderSettings,
        api_key: Option<&str>,
        call: impl AsyncFnOnce(
            Store<HostState>,
            ProviderPlugin,
            wit::Settings,
        ) -> wasmtime::Result<Result<T, types::Failure>>,
    ) -> Result<T, Failure> {
        let operation = async {
            let (store, plugin) = self.instance().await?;
            let settings = wit::Settings {
                base_url: settings.base_url.clone(),
                model: settings.model.clone(),
                api_key: api_key.map(str::to_owned),
                options: settings.options.to_string(),
            };
            call(store, plugin, settings).await
        };
        let result = tokio::time::timeout(limits::PROVIDER_CALL_TIMEOUT, operation)
            .await
            .map_err(|_| Failure {
                code: code::TIMEOUT.into(),
                message: "provider invocation timed out".into(),
                retryable: true,
            })?
            .map_err(|error| convert::bad(format!("plugin call failed: {error}")))?;
        result.map_err(convert::failure)
    }
}

#[async_trait]
impl Provider for WasmPlugin {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        api_key: Option<&str>,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        let request = convert::request(request);
        let result = self
            .invoke(
                settings,
                api_key,
                async move |mut store, plugin, settings| {
                    store
                        .run_concurrent(async move |accessor| {
                            plugin
                                .enco_plugin_provider()
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
        settings: &ProviderSettings,
        api_key: Option<&str>,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        self.invoke(
            settings,
            api_key,
            async move |mut store, plugin, settings| {
                store
                    .run_concurrent(async move |accessor| {
                        plugin
                            .enco_plugin_provider()
                            .call_embed(accessor, settings, inputs)
                            .await
                    })
                    .await?
            },
        )
        .await
    }
}
