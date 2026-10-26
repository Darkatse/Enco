//! What opening the registry establishes from the factory and restores from durable records.
use super::{active, loaded, open, promote, register_source};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{path::PathBuf, sync::Arc};

#[tokio::test]
async fn factory_records_need_registration_to_activate_and_preserve_source_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in [
        b"factory-1".as_slice(),
        b"factory-2",
        b"factory-3",
        b"owner",
    ] {
        runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let registry = open_registry(
        registry_deps(store.clone(), runtime.clone(), b"factory-1"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    let first = active(&registry).await;
    drop(registry);
    let registry = open_registry(
        registry_deps(store.clone(), runtime.clone(), b"factory-1"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    assert_eq!(active(&registry).await, first);
    drop(registry);
    let registry = open_registry(
        registry_deps(store.clone(), runtime.clone(), b"factory-2"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    assert_eq!(active(&registry).await, first);
    submit(
        &registry,
        RegistrationRequest {
            initiator: Initiator::Owner { reason: None },
            plugins: [("fixture".parse().unwrap(), PluginChange::Factory)].into(),
            base: None,
            configuration: None,
        },
    )
    .await
    .unwrap();
    let second = active(&registry).await;
    assert_eq!(store.registry().await.unwrap().generations.len(), 3);
    assert_eq!(second.origin, Origin::Factory);
    assert!(second.id > first.id);
    let owner_code = submit(&registry, source_request("fixture", b"owner"))
        .await
        .unwrap()
        .generations
        .remove(0);
    drop(registry);
    let registry = open_registry(
        registry_deps(store.clone(), runtime.clone(), b"factory-3"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    assert_eq!(active(&registry).await, owner_code);
    assert_eq!(store.registry().await.unwrap().generations.len(), 5);
    drop(registry);
    // A contract upgrade can make every older artifact unloadable, including the active one.
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(b"factory-4", loaded(ScriptedProvider::new(vec![])));
    let registry = open_registry(
        registry_deps(store.clone(), runtime.clone(), b"factory-4"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    assert_eq!(
        active(&registry).await.artifact,
        ContentHash::of(b"factory-4")
    );
    drop(registry);
    let mut deps = registry_deps(store, runtime, b"factory-4");
    deps.factory[0].id = PluginId::new();
    assert!(matches!(
        open_registry(deps, configuration_request(configuration())).await,
        Err(RegistryError::FactoryIdentity(_))
    ));
}

#[tokio::test]
async fn overlapping_table_owners_are_rejected_before_any_generation_is_written() {
    for table in ["owner", "absent"] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
        let owners = if table == "owner" {
            vec![TableOwner {
                tables: &[enco_kernel::ConfigTable::Single("owner")],
                declare: |_, _| Ok(vec![]),
            }]
        } else {
            vec![
                TableOwner {
                    tables: &[enco_kernel::ConfigTable::Single("absent")],
                    declare: |_, _| Ok(vec![]),
                },
                TableOwner {
                    tables: &[enco_kernel::ConfigTable::Single("absent")],
                    declare: |_, _| Ok(vec![]),
                },
            ]
        };
        let result = open_registry(
            RegistryDeps {
                owners,
                ..registry_deps(
                    store.clone(),
                    Arc::new(ScriptedRuntime::default()),
                    FACTORY_BYTES,
                )
            },
            configuration_request(configuration()),
        )
        .await;
        assert!(matches!(result, Err(RegistryError::TableOwner(name)) if name == table));
        let records = store.registry().await.unwrap();
        assert!(records.generations.is_empty() && records.config_generations.is_empty());
    }
}

/// Durable active state left by a process; `older` is the healthy generation before it.
struct Crashed {
    dir: tempfile::TempDir,
    store: Arc<SqliteStore>,
    runtime: Arc<ScriptedRuntime>,
    factory: GenerationId,
    latest: GenerationId,
    older_artifact: ContentHash,
    latest_artifact: ContentHash,
}

impl Crashed {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
        let runtime = Arc::new(ScriptedRuntime::default());
        for bytes in [FACTORY_BYTES, b"older", b"latest"] {
            runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
        }
        let registry = open(&store, runtime.clone()).await;
        let factory = active(&registry).await.id;
        let older = register_source(&registry, "fixture", b"older").await;
        promote(&registry, older.id).await;
        let latest = register_source(&registry, "fixture", b"latest").await;
        promote(&registry, latest.id).await;
        drop(registry);
        Self {
            dir,
            store,
            runtime,
            factory,
            latest: latest.id,
            older_artifact: older.artifact,
            latest_artifact: latest.artifact,
        }
    }

    fn artifact(&self, hash: &ContentHash) -> PathBuf {
        self.dir
            .path()
            .join("artifacts")
            .join(format!("{hash}.wasm"))
    }

    async fn open(&self) -> Arc<Registry> {
        open(&self.store, self.runtime.clone()).await
    }

    /// Loses the committed artifact and corrupts the one before it, then reopens.
    async fn lose_artifacts(&self) -> Arc<Registry> {
        std::fs::remove_file(self.artifact(&self.latest_artifact)).unwrap();
        std::fs::write(self.artifact(&self.older_artifact), b"corrupt").unwrap();
        self.open().await
    }
}

#[tokio::test]
async fn recovery_uses_the_committed_activation() {
    let crashed = Crashed::new().await;
    let registry = crashed.open().await;
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("fixture"), false)
            .unwrap()
            .generation,
        crashed.latest
    );
}

#[tokio::test]
async fn recovery_skips_missing_and_corrupt_artifacts() {
    let crashed = Crashed::new().await;
    let registry = crashed.lose_artifacts().await;
    assert_eq!(active(&registry).await.id, crashed.factory);
    for record in crashed
        .store
        .registry()
        .await
        .unwrap()
        .generations
        .values()
        .filter(|record| {
            record.plugin == FIXTURE_ID.parse().unwrap() && record.id != crashed.factory
        })
    {
        assert_eq!(record.status, GenerationStatus::Failed);
        let failure = record.failure.as_ref().unwrap();
        assert_eq!(failure.code, code::PLUGIN_LOAD);
        assert!(failure.message.contains(&record.artifact.to_string()));
    }
}

#[tokio::test]
async fn rollback_and_recovery_skip_lost_artifacts_of_a_registered_plugin() {
    let crashed = Crashed::new().await;
    let registry = crashed.lose_artifacts().await;
    let intended = registry.registered_intent(None).await.unwrap().plugins["fixture"]
        .activation
        .generation
        .clone();
    let only = register_source(&registry, "custom", b"latest").await;
    promote(&registry, only.id).await;
    let newer = register_source(&registry, "custom", b"older").await;
    assert_eq!(
        crashed.store.artifact(&newer.artifact).await.unwrap(),
        b"older"
    );
    std::fs::remove_file(crashed.artifact(&only.artifact)).unwrap();
    registry.rollback().await.unwrap();
    assert!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .is_err()
    );
    drop(registry);
    // Both custom artifacts are now unavailable; recovery leaves it registered without exports.
    let registry = crashed.open().await;
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .err()
            .unwrap()
            .code,
        code::PLUGIN_UNAVAILABLE
    );
    let restored = registry.registered_intent(None).await.unwrap();
    assert_eq!(restored.plugins["fixture"].activation.generation, intended);
    let history = crashed.store.registry().await.unwrap();
    let failed = &history.generations[&only.id];
    assert_eq!(failed.failure.as_ref().unwrap().code, code::PLUGIN_LOAD);
    let retired = &history.generations[&newer.id];
    assert_eq!(retired.status, GenerationStatus::Trial);
    assert!(retired.failure.is_none());
}

#[tokio::test]
async fn registering_new_factory_code_preserves_rollback_content() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    runtime.insert(FACTORY_BYTES, loaded(provider.clone()));
    runtime.insert(b"new-factory", loaded(provider));
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let registry = super::open(&store, runtime.clone()).await;
    let mut document = configuration();
    document["plugin"] = serde_json::json!({"fixture": {"config": {"label": "good"}}});
    submit(&registry, configuration_request(document.clone()))
        .await
        .unwrap();
    let good = active(&registry).await.id;
    for _ in 0..TRIAL_CALLS {
        registry.report(good, Verdict::Ok, None).await.unwrap();
    }
    document["plugin"]["fixture"]["config"]["label"] = "bad".into();
    submit(&registry, configuration_request(document))
        .await
        .unwrap();
    registry
        .report(
            active(&registry).await.id,
            Verdict::Failed(super::fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(active(&registry).await.id, good);
    drop(registry);
    let registry = open_registry(
        registry_deps(store, runtime, b"new-factory"),
        configuration_request(configuration()),
    )
    .await
    .unwrap();
    submit(
        &registry,
        RegistrationRequest {
            initiator: Initiator::Owner { reason: None },
            plugins: [("fixture".parse().unwrap(), PluginChange::Factory)].into(),
            base: None,
            configuration: None,
        },
    )
    .await
    .unwrap();
    let upgraded = active(&registry).await;
    assert_eq!(upgraded.artifact, ContentHash::of(b"new-factory"));
    assert_eq!(upgraded.settings.config, serde_json::json!({"label":"bad"}));
    let factory = registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), true)
        .unwrap()
        .generation;
    assert_ne!(upgraded.id, factory);
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .rollback_target
            .unwrap()
            .generation,
        good
    );
    registry
        .report(
            upgraded.id,
            Verdict::Failed(super::fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(active(&registry).await.id, good);
}

#[tokio::test]
async fn rejected_settings_do_not_register_either_plugin_or_configuration() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let registry = super::open(&store, runtime).await;
    let before = store.registry().await.unwrap();
    let mut document = configuration();
    document["plugin"] = serde_json::json!({"fixture": {"config": {"label": "good"}}});
    let sql = rusqlite::Connection::open(root.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_settings BEFORE INSERT ON plugin_generations BEGIN SELECT RAISE(FAIL, 'reject settings'); END").unwrap();
    assert!(
        submit(&registry, configuration_request(document.clone()))
            .await
            .is_err()
    );
    let rejected = store.registry().await.unwrap();
    assert_eq!(rejected.config_generations, before.config_generations);
    assert_eq!(rejected.generations, before.generations);
    assert_eq!(
        registry.view().config.generation,
        before.active_config.unwrap()
    );
    sql.execute_batch("DROP TRIGGER reject_settings").unwrap();
}
