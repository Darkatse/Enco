use crate::{
    bindings::{CompletionPluginPre, EmbeddingPluginPre, enco::plugin::host},
    engine::WasmEngine,
    host_imports::HostState,
    plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::ContentHash;
use enco_kernel::{Embedding, LoadError, Loaded, Provider, Runtime};
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
        host::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |s| s).map_err(load_error)?;
        let pre = linker.instantiate_pre(&component).map_err(load_error)?;
        let completion = component
            .get_export_index(None, "enco:plugin/completion@0.2.0")
            .map(|_| CompletionPluginPre::new(pre.clone()))
            .transpose()
            .map_err(load_error)?;
        let embedding = component
            .get_export_index(None, "enco:plugin/embedding@0.2.0")
            .map(|_| EmbeddingPluginPre::new(pre))
            .transpose()
            .map_err(load_error)?;
        if completion.is_none() && embedding.is_none() {
            return Err(LoadError(
                "component exports neither completion nor embedding".into(),
            ));
        }
        let plugin = Arc::new(WasmPlugin {
            completion,
            embedding,
            engine: self.engine.clone(),
            http: self.http.clone(),
            name: hash.to_string()[..8].into(),
        });
        let summary = plugin
            .describe(config)
            .await
            .map_err(|failure| LoadError(format!("{}: {}", failure.code, failure.message)))?;
        Ok(Loaded {
            summary,
            completion: plugin
                .completion
                .is_some()
                .then(|| plugin.clone() as Arc<dyn Provider>),
            embedding: plugin
                .embedding
                .is_some()
                .then(|| plugin.clone() as Arc<dyn Embedding>),
        })
    }
}

fn load_error(error: impl std::fmt::Display) -> LoadError {
    LoadError(error.to_string())
}
