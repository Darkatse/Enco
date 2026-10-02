use crate::{
    bindings::{ProviderPlugin, ProviderPluginPre},
    engine::WasmEngine,
    host_imports::HostState,
    limits,
    plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::ContentHash;
use enco_kernel::{LoadError, Loaded, Runtime};
use std::sync::Arc;
use wasmtime::component::{Component, HasSelf, Linker};

/// Shared engine and transport; invocation parameters belong to callers, not compiled code.
pub struct WasmRuntime {
    engine: Arc<WasmEngine>,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("Wasm engine: {0}")]
    Engine(wasmtime::Error),
    #[error("HTTP client: {0}")]
    Http(#[from] reqwest::Error),
}

impl WasmRuntime {
    pub fn new() -> Result<Self, WasmError> {
        Ok(Self {
            engine: Arc::new(WasmEngine::new()?),
            http: reqwest::Client::builder().build()?,
        })
    }
}

#[async_trait]
impl Runtime for WasmRuntime {
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError> {
        let hash = ContentHash::of(artifact);
        let bytes = artifact.to_vec();
        let engine = self.engine.engine.clone();
        let component = tokio::task::spawn_blocking(move || Component::new(&engine, &bytes))
            .await
            .map_err(load_error)?
            .map_err(load_error)?;
        let mut linker = Linker::new(&self.engine.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(load_error)?;
        ProviderPlugin::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |s| s)
            .map_err(load_error)?;
        let pre = ProviderPluginPre::new(linker.instantiate_pre(&component).map_err(load_error)?)
            .map_err(load_error)?;
        let plugin = Arc::new(WasmPlugin {
            pre,
            engine: self.engine.clone(),
            http: self.http.clone(),
            name: hash.to_string()[..8].into(),
        });
        let info = tokio::time::timeout(limits::PROVIDER_CALL_TIMEOUT, async {
            let (mut store, instance) = plugin.instance().await?;
            instance
                .enco_plugin_lifecycle()
                .call_describe(&mut store, &config.to_string())
                .await
        })
        .await
        .map_err(load_error)?
        .map_err(load_error)?;
        // WIT 0.1 exports both operations together. M12 discovers the separate 0.2 interfaces.
        Ok(Loaded {
            summary: format!("{} {}", info.name, info.version),
            completion: Some(plugin.clone()),
            embedding: Some(plugin),
        })
    }
}

fn load_error(error: impl std::fmt::Display) -> LoadError {
    LoadError(error.to_string())
}
