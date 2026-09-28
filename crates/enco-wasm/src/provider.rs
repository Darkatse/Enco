use crate::{
    WasmEngine,
    bindings::{
        ProviderPlugin, ProviderPluginPre, enco::plugin::types,
        exports::enco::plugin::provider as wit,
    },
    convert,
    host_imports::HostState,
    limits,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{Completion, Provider, ProviderRequest};
use std::sync::Arc;
use wasmtime::{
    Store, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker, ResourceTable},
};
use wasmtime_wasi::WasiCtx;

/// Adapter settings; credentials are never written into a ContextPlan or Log.
#[derive(Clone)]
pub struct ProviderSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub options: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("Wasm component: {0}")]
    Runtime(wasmtime::Error),
    #[error("Wasm setup timed out")]
    Timeout,
    #[error("HTTP client: {0}")]
    Http(#[from] reqwest::Error),
}

/// A compiled component plus settings. Each invocation owns a fresh Store until settlement.
pub struct WasmProvider {
    pre: ProviderPluginPre<HostState>,
    engine: Arc<WasmEngine>,
    artifact: ContentHash,
    settings: ProviderSettings,
    http: reqwest::Client,
    name: String,
}

impl WasmProvider {
    pub async fn new(
        engine: Arc<WasmEngine>,
        component_bytes: &[u8],
        settings: ProviderSettings,
    ) -> Result<Self, WasmError> {
        let component =
            Component::new(&engine.engine, component_bytes).map_err(WasmError::Runtime)?;
        let mut linker = Linker::new(&engine.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(WasmError::Runtime)?;
        ProviderPlugin::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |s| s)
            .map_err(WasmError::Runtime)?;
        let pre = ProviderPluginPre::new(
            linker
                .instantiate_pre(&component)
                .map_err(WasmError::Runtime)?,
        )
        .map_err(WasmError::Runtime)?;
        let mut provider = Self {
            pre,
            engine,
            artifact: ContentHash::of(component_bytes),
            settings,
            http: reqwest::Client::builder().build()?,
            name: "initializing".into(),
        };
        let info = tokio::time::timeout(limits::PROVIDER_CALL_TIMEOUT, async {
            let (mut store, plugin) = provider.instance().await?;
            plugin
                .enco_plugin_lifecycle()
                .call_describe(&mut store, &"{}".to_string())
                .await
        })
        .await
        .map_err(|_| WasmError::Timeout)?
        .map_err(WasmError::Runtime)?;
        provider.name = info.name;
        Ok(provider)
    }

    async fn instance(&self) -> wasmtime::Result<(Store<HostState>, ProviderPlugin)> {
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
        call: impl AsyncFnOnce(
            Store<HostState>,
            ProviderPlugin,
            wit::Settings,
        ) -> wasmtime::Result<Result<T, types::Failure>>,
    ) -> Result<T, Failure> {
        let operation = async {
            let (store, plugin) = self.instance().await?;
            let settings = wit::Settings {
                base_url: self.settings.base_url.clone(),
                model: self.settings.model.clone(),
                api_key: self.settings.api_key.clone(),
                options: self.settings.options.to_string(),
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
impl Provider for WasmProvider {
    fn code(&self) -> CodeRef {
        CodeRef::Wasm {
            artifact: self.artifact,
        }
    }

    async fn complete(&self, request: ProviderRequest) -> Result<Completion, Failure> {
        let request = convert::request(request);
        let result = self
            .invoke(async move |mut store, plugin, settings| {
                store
                    .run_concurrent(async move |accessor| {
                        plugin
                            .enco_plugin_provider()
                            .call_complete(accessor, settings, request)
                            .await
                    })
                    .await?
            })
            .await?;
        convert::completion(result)
    }

    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        self.invoke(async move |mut store, plugin, settings| {
            store
                .run_concurrent(async move |accessor| {
                    plugin
                        .enco_plugin_provider()
                        .call_embed(accessor, settings, inputs)
                        .await
                })
                .await?
        })
        .await
    }
}
