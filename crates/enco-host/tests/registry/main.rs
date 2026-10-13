#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
#[path = "../support/mod.rs"]
mod support;

mod admission;
mod callers;
mod opening;
mod races;
mod trial;

use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::sync::Arc;
use support::*;

fn loaded(provider: Arc<ScriptedProvider>) -> Loaded {
    fixture(provider.clone(), provider)
}

async fn promote(registry: &Registry, generation: GenerationId) {
    for _ in 0..TRIAL_CALLS {
        registry
            .report(generation, Verdict::Ok, None)
            .await
            .unwrap();
    }
}

fn fault(code: &str) -> Failure {
    Failure {
        code: code.into(),
        message: "fixture failure".into(),
        retryable: false,
    }
}

/// Opens the registry over the scripted factory and the default configuration.
async fn open(store: &Arc<SqliteStore>, runtime: Arc<dyn Runtime>) -> Arc<Registry> {
    Registry::open(registry_deps(
        store.clone(),
        runtime,
        FACTORY_BYTES,
        configuration(),
    ))
    .await
    .unwrap()
}

async fn deploy_artifact(registry: &Registry, name: &str, bytes: &[u8]) -> GenerationRecord {
    registry
        .deploy(name, PluginSource::Artifact(bytes.to_vec()))
        .await
        .unwrap()
        .generation
}
