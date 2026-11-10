#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support;

mod admission;
mod callers;
mod composer;
mod opening;
mod races;
mod registrations;
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
            .report(&generation.into(), Verdict::Ok, None)
            .await
            .unwrap();
    }
}

/// Every row of the tables holding accepted intent, which only a registration writes.
fn intent_tables(root: &std::path::Path) -> Vec<String> {
    let db = rusqlite::Connection::open(root.join("enco.db")).unwrap();
    let mut rows = Vec::new();
    for table in [
        "registrations",
        "registration_names",
        "registration_code",
        "plugin_generations",
        "config_generations",
    ] {
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = query.column_count();
        let mut found = query.query([]).unwrap();
        while let Some(row) = found.next().unwrap() {
            let values: Vec<String> = (0..columns)
                .map(|column| format!("{:?}", row.get_ref(column).unwrap()))
                .collect();
            rows.push(format!("{table}: {}", values.join(" | ")));
        }
    }
    rows
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
    open_registry(
        registry_deps(store.clone(), runtime, FACTORY_BYTES),
        configuration_request(configuration()),
    )
    .await
    .unwrap()
}

/// The first plugin's running code, with its content and standing on this node.
async fn active_activation(registry: &Registry) -> Activation {
    running_activation(&registry.status().await.unwrap().plugins[0])
        .unwrap()
        .clone()
}

/// The first plugin's running code.
async fn active(registry: &Registry) -> CodeRef {
    running_code(&registry.status().await.unwrap().plugins[0]).unwrap()
}

async fn register_source(registry: &Registry, name: &str, bytes: &[u8]) -> GenerationRecord {
    submit(registry, source_request(name, bytes))
        .await
        .unwrap()
        .generations
        .remove(0)
}
