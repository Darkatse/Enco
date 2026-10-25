//! Turns component bytes into a loaded plugin.
//!
//! A component may import only WASI and the `enco:plugin` `host` and `types` interfaces of a
//! compatible version; any other import, `completion` included, is rejected at load.
use crate::{
    bindings::{
        BasePre, CONTRACT_VERSION, CompletionPluginPre, DecisionPluginPre, EmbeddingPluginPre,
        ToolPluginPre, enco::plugin::host,
    },
    credentials,
    engine::WasmEngine,
    host_imports::HostState,
    plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::{ContentHash, Failure, PluginSettings, code};
use enco_kernel::{Credential, Decision, Embedding, LoadError, Loaded, Provider, Runtime};
use std::sync::Arc;
use wasmtime::component::{Component, HasSelf, Linker};

/// The host's plugin runtime: one engine and one HTTP client shared by every loaded plugin.
///
/// A loaded plugin keeps no endpoint or credential value; both come with each call.
pub struct WasmRuntime {
    engine: Arc<WasmEngine>,
    http: reqwest::Client,
}

/// Why the plugin runtime could not start.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    /// Wasmtime could not create the engine or its compiled-code cache.
    #[error("Wasm engine: {0}")]
    Engine(wasmtime::Error),
    /// The engine's epoch clock thread could not start.
    #[error("Wasm epoch clock: {0}")]
    Clock(std::io::Error),
    /// The HTTP client could not be built.
    #[error("HTTP client: {0}")]
    Http(#[from] reqwest::Error),
}

impl WasmRuntime {
    /// Start the shared engine and its epoch clock thread, and build the HTTP client.
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
    async fn load(&self, artifact: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError> {
        let hash = ContentHash::of(artifact);
        let bytes = artifact.to_vec();
        let engine = self.engine.engine.clone();
        let component = tokio::task::spawn_blocking(move || Component::new(&engine, &bytes))
            .await
            .map_err(load_error)?
            .map_err(load_error)?;
        check_imports(&component, &self.engine.engine)?;
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
            .then(|| DecisionPluginPre::new(pre.clone()))
            .transpose()
            .map_err(load_error)?;
        let tools = exports("tools")
            .then(|| ToolPluginPre::new(pre))
            .transpose()
            .map_err(load_error)?;
        if completion.is_none() && embedding.is_none() && decision.is_none() && tools.is_none() {
            return Err(LoadError::Component(
                "component exports none of completion, embedding, decision or tools".into(),
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
            settings: settings.clone(),
            placements: Vec::new(),
        };
        let description = plugin
            .describe(&settings.config)
            .await
            .map_err(describe_error)?;
        let config_schema = serde_json::from_str(&description.config_schema)
            .map_err(|e| LoadError::Component(format!("configuration schema is not JSON: {e}")))?;
        plugin.placements = credentials::placements(&description.credentials, settings)?;
        let unavailable = credentials::resolve(&plugin.placements, settings, None).err();
        let plugin = Arc::new(plugin);
        let tools = match tools {
            Some(pre) => Some(
                crate::tools::load(plugin.clone(), pre)
                    .await
                    .map_err(LoadError::Plugin)?,
            ),
            None => None,
        };
        Ok(Loaded {
            summary: description.summary,
            config_schema,
            credentials: description
                .credentials
                .into_iter()
                .map(|credential| Credential {
                    name: credential.name,
                    origin: credential.origin,
                    required: credential.required,
                    header: credential.header,
                    prefix: credential.prefix,
                })
                .collect(),
            unavailable,
            tools,
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

/// A component that rejects its configuration reports `config.invalid` from `describe`.
fn describe_error(failure: Failure) -> LoadError {
    if failure.code == code::CONFIG_INVALID {
        LoadError::Settings(failure.message)
    } else {
        LoadError::Plugin(failure)
    }
}

fn load_error(error: impl std::fmt::Display) -> LoadError {
    LoadError::Component(error.to_string())
}

fn check_imports(component: &Component, engine: &wasmtime::Engine) -> Result<(), LoadError> {
    for (name, _) in component.component_type().imports(engine) {
        if name.starts_with("enco:plugin/completion@") {
            return Err(LoadError::Component("plugins cannot import completion; model calls must be recorded as Attempts by the kernel".into()));
        }
        if let Some(version) = name
            .strip_prefix("enco:plugin/host@")
            .or_else(|| name.strip_prefix("enco:plugin/types@"))
        {
            if !same_major(version) {
                return Err(LoadError::Component(format!(
                    "built against enco:plugin@{version}; this host links enco:plugin@{CONTRACT_VERSION}, rebuild the plugin"
                )));
            }
        } else if !name.starts_with("wasi:") {
            return Err(LoadError::Component(format!(
                "unsupported plugin import {name}; only host interfaces are available"
            )));
        }
    }
    Ok(())
}
