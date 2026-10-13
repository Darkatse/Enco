//! What admission lets become a generation, and what a rejection leaves behind.
use super::{fault, loaded, open};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::sync::Arc;

#[tokio::test]
async fn admission_keeps_configured_exports_connected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    let mut factory = loaded(provider.clone());
    factory.credential = Some(Credential { required: false });
    runtime.insert(FACTORY_BYTES, factory);
    runtime.insert(b"no-credential", loaded(provider.clone()));
    let mut required = loaded(provider.clone());
    required.credential = Some(Credential { required: true });
    runtime.insert(b"required", required);
    runtime.insert(
        b"completion-only",
        Loaded {
            lifecycle: Arc::new(ScriptedLifecycle(Ok(()))),
            summary: "completion".into(),
            config_schema: serde_json::json!({}),
            credential: None,
            completion: Some(provider),
            embedding: None,
            decision: None,
        },
    );
    let mut document = configuration();
    document["endpoint"]["embed"]["api_key_env"] = "FIXTURE_KEY".into();
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        FACTORY_BYTES,
        document,
    ))
    .await
    .unwrap();
    let before = registry.status().await.plugins.remove(0);
    for (bytes, user, explanation) in [
        (
            b"completion-only".as_slice(),
            "memory.embedding",
            "requires embedding",
        ),
        (
            b"no-credential",
            "memory.embedding",
            "declares no credential",
        ),
        (
            b"required",
            "profile default.reply",
            "requires a credential",
        ),
    ] {
        let result = registry
            .deploy(
                "fixture",
                enco_kernel::PluginSource::Artifact(bytes.to_vec()),
            )
            .await;
        let Err(RegistryError::Rejected { name, reason }) = result else {
            panic!("expected admission rejection");
        };
        assert_eq!(name, "fixture");
        assert!(
            reason.contains(user) && reason.contains(explanation),
            "{reason}"
        );
    }
    assert_eq!(
        registry.status().await.plugins[0].generations,
        before.generations
    );
    drop(registry);
    // Restoring registered content reports unavailable uses instead of accepting new intent.
    let restored = Registry::open(registry_deps(
        store,
        runtime.clone(),
        b"required",
        serde_json::json!({"ignored": true}),
    ))
    .await
    .unwrap();
    assert!(restored.view().config.document.get("memory").is_some());
    let fresh = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(fresh.path())).await.unwrap());
    let mut document = configuration();
    document["endpoint"]["reply"]["plugin"] = "missing".into();
    let result = Registry::open(registry_deps(store, runtime, FACTORY_BYTES, document)).await;
    let Err(RegistryError::Rejected { name, reason }) = result else {
        panic!("expected missing plugin rejection");
    };
    assert_eq!(name, "missing");
    assert!(
        reason.contains("profile default.reply") && reason.contains("not registered"),
        "{reason}"
    );
}

#[tokio::test]
async fn a_rejected_deployment_writes_neither_a_generation_nor_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    runtime.insert(FACTORY_BYTES, loaded(provider.clone()));
    let mut rejected = loaded(provider);
    rejected.lifecycle = Arc::new(ScriptedLifecycle(Err(fault(code::PLUGIN_TRAP))));
    runtime.insert(b"rejected", rejected);
    let registry = open(&store, runtime).await;
    let before = store.registry().await.unwrap().generations;
    assert!(matches!(
        registry
            .deploy(
                "new-name",
                enco_kernel::PluginSource::Artifact(b"rejected".to_vec())
            )
            .await,
        Err(RegistryError::Rejected { .. })
    ));
    assert_eq!(store.registry().await.unwrap().generations, before);
    assert!(!store.plugin_names().await.unwrap().contains_key("new-name"));
}
