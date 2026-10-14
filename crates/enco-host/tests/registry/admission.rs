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
            unavailable: None,
            tools: None,
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
}

#[tokio::test]
async fn unavailable_uses_are_isolated_and_can_be_repaired_through_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    let mut optional = loaded(provider.clone());
    optional.credential = Some(Credential { required: false });
    runtime.insert(FACTORY_BYTES, optional);
    runtime.insert(b"no-credential", loaded(provider));
    let mut document = configuration();
    document["endpoint"]["embed"]["api_key_env"] = "FIXTURE_KEY".into();
    drop(
        Registry::open(registry_deps(
            store.clone(),
            runtime.clone(),
            FACTORY_BYTES,
            document.clone(),
        ))
        .await
        .unwrap(),
    );
    let registry = Registry::open(registry_deps(
        store,
        runtime,
        b"no-credential",
        document.clone(),
    ))
    .await
    .unwrap();
    let status = registry.status().await;
    assert_eq!(status.plugins[0].unavailable, None);
    assert_eq!(status.plugins[0].unavailable_uses.len(), 1);
    assert!(status.plugins[0].unavailable_uses[0].contains("memory.embedding"));
    assert_eq!(
        status.config.unavailable_uses,
        status.plugins[0].unavailable_uses
    );
    let reply = fixture_endpoint("fixture");
    assert!(registry.view().exports.completion(&reply, false).is_ok());
    let mut embed = reply.clone();
    embed.name = "embed".into();
    embed.settings.api_key_env = Some("FIXTURE_KEY".into());
    assert!(registry.view().exports.embedding(&embed).is_err());

    document["owner"]["timezone"] = "Europe/Paris".into();
    registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .unwrap();
    let before = registry.view().config.generation;
    let mut invalid = document.clone();
    invalid["endpoint"]["new"] = invalid["endpoint"]["reply"].clone();
    invalid["endpoint"]["new"]["api_key_env"] = "FIXTURE_KEY".into();
    invalid["profile"]["default"]["reply"] = serde_json::json!(["reply", "new"]);
    let error = registry
        .apply_config(ConfigSource::Document(invalid))
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("declares no credential"));
    assert_eq!(registry.view().config.generation, before);

    document["endpoint"]["embed"]
        .as_object_mut()
        .unwrap()
        .remove("api_key_env");
    registry
        .apply_config(ConfigSource::Document(document))
        .await
        .unwrap();
    let status = registry.status().await;
    assert!(status.plugins[0].unavailable_uses.is_empty());
    assert!(status.config.unavailable_uses.is_empty());
    embed.settings.api_key_env = None;
    assert!(registry.view().exports.embedding(&embed).is_ok());
}

#[tokio::test]
async fn initial_configuration_requires_registered_plugins() {
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
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

#[tokio::test]
async fn unrelated_unavailable_plugins_do_not_block_incremental_repairs() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    let available = fixture(provider.clone(), provider);
    runtime.insert(FACTORY_BYTES, available.clone());
    runtime.insert(b"optional", available.clone());
    runtime.insert(b"repair", available.clone());
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let mut document = configuration();
    document["endpoint"]["optional"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["optional"]["plugin"] = "optional".into();
    document["profile"]["default"]["reply"] = serde_json::json!(["reply", "optional"]);
    let factory = FactoryPlugin {
        name: "optional".into(),
        id: PluginId::new(),
        artifact: b"optional".to_vec(),
    };
    let mut deps = registry_deps(
        store.clone(),
        runtime.clone(),
        FACTORY_BYTES,
        document.clone(),
    );
    deps.factory.push(FactoryPlugin {
        name: factory.name.clone(),
        id: factory.id,
        artifact: factory.artifact.clone(),
    });
    let registry = Registry::open(deps).await.unwrap();
    drop(registry);
    let mut unavailable = available;
    unavailable.unavailable = Some(Failure {
        code: code::CREDENTIAL_UNAVAILABLE.into(),
        message: "OPTIONAL_KEY is missing".into(),
        retryable: false,
    });
    runtime.insert(b"optional", unavailable);
    let mut deps = registry_deps(store, runtime, FACTORY_BYTES, document.clone());
    deps.factory.push(factory);
    let registry = Registry::open(deps).await.unwrap();
    registry
        .deploy("fixture", PluginSource::Artifact(b"repair".to_vec()))
        .await
        .unwrap();
    document["owner"]["timezone"] = "Europe/Paris".into();
    registry
        .apply_config(ConfigSource::Document(document))
        .await
        .unwrap();
    let status = registry.status().await;
    let optional = status
        .plugins
        .iter()
        .find(|plugin| plugin.name == "optional")
        .unwrap();
    assert!(
        optional
            .unavailable
            .as_ref()
            .unwrap()
            .contains("OPTIONAL_KEY")
    );
}
