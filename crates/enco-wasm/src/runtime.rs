use crate::{
    bindings::{
        BasePre, CONTRACT_VERSION, CompletionPluginPre, DecisionPluginPre, EmbeddingPluginPre,
        enco::plugin::host,
    },
    credentials::Placement,
    engine::WasmEngine,
    host_imports::HostState,
    plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::ContentHash;
use enco_kernel::{Credential, Decision, Embedding, LoadError, Loaded, Provider, Runtime};
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
            http: reqwest::Client::builder()
                .redirect(same_origin_redirects())
                .build()?,
        })
    }
}

/// Credentials attach by origin, while reqwest strips only a few known headers when a redirect
/// changes host. Redirects are therefore followed only within one origin; any other redirect is
/// returned to the plugin as the response.
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let leaves_origin = attempt
            .previous()
            .last()
            .is_some_and(|previous| previous.origin() != attempt.url().origin());
        if leaves_origin {
            attempt.stop()
        } else if attempt.previous().len() > crate::limits::HTTP_MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

/// In a 0.x contract the minor version is the compatibility boundary.
fn same_major(version: &str) -> bool {
    version
        .split('.')
        .take(2)
        .eq(CONTRACT_VERSION.split('.').take(2))
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
        for (name, _) in component.component_type().imports(&self.engine.engine) {
            if let Some(version) = name.strip_prefix("enco:plugin/host@")
                && !same_major(version)
            {
                return Err(LoadError(format!(
                    "built against enco:plugin@{version}; this host links enco:plugin@{CONTRACT_VERSION}, rebuild the plugin"
                )));
            }
        }
        let mut linker = Linker::new(&self.engine.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(load_error)?;
        host::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |s| s).map_err(load_error)?;
        let pre = linker.instantiate_pre(&component).map_err(load_error)?;
        let lifecycle = BasePre::new(pre.clone()).map_err(load_error)?;
        // Wasmtime handles compatible patch versions when resolving an export.
        let exports = |interface: &str| {
            component
                .get_export_index(None, format!("enco:plugin/{interface}@{CONTRACT_VERSION}"))
                .is_some()
        };
        let completion = exports("completion")
            .then(|| CompletionPluginPre::new(pre.clone()))
            .transpose()
            .map_err(load_error)?;
        let embedding = exports("embedding")
            .then(|| EmbeddingPluginPre::new(pre.clone()))
            .transpose()
            .map_err(load_error)?;
        let decision = exports("decision")
            .then(|| DecisionPluginPre::new(pre))
            .transpose()
            .map_err(load_error)?;
        if completion.is_none() && embedding.is_none() && decision.is_none() {
            return Err(LoadError(
                "component exports none of completion, embedding or decision".into(),
            ));
        }
        let mut plugin = WasmPlugin {
            lifecycle,
            completion,
            embedding,
            decision,
            engine: self.engine.clone(),
            http: self.http.clone(),
            name: hash.to_string()[..8].into(),
            placement: None,
        };
        let description = plugin
            .describe(config)
            .await
            .map_err(|failure| LoadError(format!("{}: {}", failure.code, failure.message)))?;
        let config_schema = serde_json::from_str(&description.config_schema)
            .map_err(|e| LoadError(format!("configuration schema is not JSON: {e}")))?;
        plugin.placement = description
            .credential
            .as_ref()
            .map(Placement::new)
            .transpose()?;
        let plugin = Arc::new(plugin);
        Ok(Loaded {
            summary: description.summary,
            config_schema,
            credential: description.credential.map(|credential| Credential {
                required: credential.required,
            }),
            lifecycle: plugin.clone(),
            completion: plugin
                .completion
                .is_some()
                .then(|| plugin.clone() as Arc<dyn Provider>),
            embedding: plugin
                .embedding
                .is_some()
                .then(|| plugin.clone() as Arc<dyn Embedding>),
            decision: plugin
                .decision
                .is_some()
                .then(|| plugin.clone() as Arc<dyn Decision>),
        })
    }
}

fn load_error(error: impl std::fmt::Display) -> LoadError {
    LoadError(error.to_string())
}
