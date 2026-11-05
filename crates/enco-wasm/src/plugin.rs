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
    credentials::Credential,
    engine::WasmEngine,
    host_imports::{HostState, Network},
    limits,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::Lifecycle;
use std::sync::Arc;
use wasmtime::{Store, StoreLimitsBuilder, component::ResourceTable};

/// Compiled exports with no captured endpoint or credential value.
pub(crate) struct WasmPlugin {
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
    ///
    /// A call that runs out of time is a retryable `timeout` when its world has network, and
    /// `plugin.trap` otherwise.
    pub(super) async fn invoke<T>(
        &self,
        world: CallWorld,
        cancel: tokio_util::sync::CancellationToken,
        call: impl AsyncFnOnce(Store<HostState>) -> wasmtime::Result<Result<T, types::Failure>>,
    ) -> Result<T, Failure> {
        let (network, config, wasi) = world.configure(&self.settings.config)?;
        // Without network a call waits on nothing but its own code, so running out of time is that
        // code's fault and repeating the call meets it again.
        let timeout = match network {
            Network::Open(_) => Failure {
                code: code::TIMEOUT.into(),
                message: "plugin invocation timed out".into(),
                retryable: true,
            },
            Network::Closed => trap(format!(
                "plugin code did not finish within {} s",
                limits::PLUGIN_CALL_TIMEOUT.as_secs()
            )),
        };
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
            .map_err(|_| timeout)?
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

/// The invocation itself failed, as opposed to the guest answering.
pub(crate) fn trap(message: impl Into<String>) -> Failure {
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
