//! What opening the registry establishes from the factory and restores from durable records.
use super::{deploy_artifact, loaded, open, promote};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{path::PathBuf, sync::Arc};

#[tokio::test]
async fn factory_updates_are_idempotent_and_preserve_owner_deployments() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in [b"factory-1", b"factory-2", b"factory-3"] {
        runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        b"factory-1",
        configuration(),
    ))
    .await
    .unwrap();
    let first = registry.status().await.plugins.remove(0);
    drop(registry);
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        b"factory-1",
        configuration(),
    ))
    .await
    .unwrap();
    assert_eq!(
        registry.status().await.plugins[0].generations,
        first.generations
    );
    drop(registry);
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        b"factory-2",
        configuration(),
    ))
    .await
    .unwrap();
    let second = registry.status().await.plugins.remove(0);
    assert_eq!(second.generations.len(), 2);
    assert_eq!(second.active.as_ref().unwrap().origin, Origin::Factory);
    assert!(second.active.unwrap().id > first.active.unwrap().id);
    let deployed = deploy_artifact(&registry, "fixture", b"factory-1").await;
    drop(registry);
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        b"factory-3",
        configuration(),
    ))
    .await
    .unwrap();
    assert_eq!(
        registry.status().await.plugins[0].active.as_ref(),
        Some(&deployed)
    );
    assert_eq!(registry.status().await.plugins[0].generations.len(), 4);
    drop(registry);
    // A contract upgrade can make every older artifact unloadable, including the active one.
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(b"factory-4", loaded(ScriptedProvider::new(vec![])));
    let registry = Registry::open(registry_deps(
        store.clone(),
        runtime.clone(),
        b"factory-4",
        configuration(),
    ))
    .await
    .unwrap();
    assert_eq!(
        registry.status().await.plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"factory-4")
    );
    drop(registry);
    std::fs::write(
        dir.path().join("plugins.lock"),
        format!("[plugins.fixture]\nid = \"{}\"\n", PluginId::new()),
    )
    .unwrap();
    assert!(matches!(
        Registry::open(registry_deps(store, runtime, b"factory-4", configuration())).await,
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
                tables: &["owner"],
                declare: |_, _| Ok(vec![]),
            }]
        } else {
            vec![
                TableOwner {
                    tables: &["absent"],
                    declare: |_, _| Ok(vec![]),
                },
                TableOwner {
                    tables: &["absent"],
                    declare: |_, _| Ok(vec![]),
                },
            ]
        };
        let result = Registry::open(RegistryDeps {
            owners,
            ..registry_deps(
                store.clone(),
                Arc::new(ScriptedRuntime::default()),
                FACTORY_BYTES,
                configuration(),
            )
        })
        .await;
        assert!(matches!(result, Err(RegistryError::TableOwner(name)) if name == table));
        let records = store.registry().await.unwrap();
        assert!(records.generations.is_empty() && records.config_generations.is_empty());
    }
}

/// The fixture plugin after a process committed `latest` active and crashed before publishing
/// it; `older` is the healthy generation before it.
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
        let factory = registry.status().await.plugins.remove(0).active.unwrap().id;
        let older = deploy_artifact(&registry, "fixture", b"older").await;
        promote(&registry, older.id).await;
        // Emulate a crash after the durable commit but before in-memory publication.
        let pending = NewGeneration {
            plugin: older.plugin,
            artifact: store.put_artifact(b"latest").await.unwrap(),
            settings: PluginSettings::default(),
            origin: Origin::Deployed,
            status: GenerationStatus::Healthy,
            created_at: SystemClock.now(),
        };
        let latest = store.insert_generation(&pending, true).await.unwrap();
        Self {
            dir,
            store,
            runtime,
            factory,
            latest,
            older_artifact: older.artifact,
            latest_artifact: pending.artifact,
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
    let status = registry.status().await.plugins.remove(0);
    assert_eq!(status.active.as_ref().unwrap().id, crashed.factory);
    for record in status
        .generations
        .iter()
        .filter(|record| record.id != crashed.factory)
    {
        assert_eq!(record.status, GenerationStatus::Failed);
        let failure = record.failure.as_ref().unwrap();
        assert_eq!(failure.code, code::PLUGIN_LOAD);
        assert!(failure.message.contains(&record.artifact.to_string()));
    }
}

#[tokio::test]
async fn rollback_and_recovery_skip_lost_artifacts_of_a_deployed_plugin() {
    let crashed = Crashed::new().await;
    let registry = crashed.lose_artifacts().await;
    let status = registry.status().await.plugins.remove(0);
    let only = deploy_artifact(&registry, "custom", b"latest").await;
    promote(&registry, only.id).await;
    let newer = deploy_artifact(&registry, "custom", b"older").await;
    assert_eq!(
        crashed.store.artifact(&newer.artifact).await.unwrap(),
        b"older"
    );
    std::fs::remove_file(crashed.artifact(&only.artifact)).unwrap();
    assert!(matches!(
        registry.rollback("custom").await,
        Err(RegistryError::NoRollbackTarget(_))
    ));
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .generation,
        newer.id
    );
    drop(registry);
    std::fs::remove_file(crashed.artifact(&newer.artifact)).unwrap();
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
    let restored = registry.status().await.plugins;
    assert_eq!(
        restored
            .iter()
            .find(|plugin| plugin.name == "fixture")
            .unwrap()
            .generations,
        status.generations
    );
    let custom = restored
        .iter()
        .find(|plugin| plugin.name == "custom")
        .unwrap();
    for record in &custom.generations {
        assert_eq!(record.failure.as_ref().unwrap().code, code::PLUGIN_LOAD);
    }
}

#[tokio::test]
async fn setting_registration_is_atomic_and_factory_following_preserves_rollback_content() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![]);
    runtime.insert(FACTORY_BYTES, loaded(provider.clone()));
    runtime.insert(b"new-factory", loaded(provider));
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let registry = super::open(&store, runtime.clone()).await;
    let before = store.registry().await.unwrap();
    let mut document = configuration();
    document["plugin"] = serde_json::json!({"fixture": {"config": {"label": "good"}}});
    let sql = rusqlite::Connection::open(root.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_settings BEFORE INSERT ON plugin_generations BEGIN SELECT RAISE(FAIL, 'reject settings'); END").unwrap();
    assert!(
        registry
            .apply_config(ConfigSource::Document(document.clone()))
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
    registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .unwrap();
    let good = registry.status().await.plugins.remove(0).active.unwrap().id;
    for _ in 0..TRIAL_CALLS {
        registry.report(good, Verdict::Ok, None).await.unwrap();
    }
    document["plugin"]["fixture"]["config"]["label"] = "bad".into();
    registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .unwrap();
    registry
        .report(
            registry.status().await.plugins.remove(0).active.unwrap().id,
            Verdict::Failed(super::fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.plugins.remove(0).active.unwrap().id,
        good
    );
    document["owner"]["timezone"] = "Europe/Paris".into();
    registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.plugins.remove(0).active.unwrap().id,
        good
    );
    assert!(matches!(
        registry.config_divergence(Ok(document)).await,
        ConfigDivergence::PluginSettings { .. }
    ));
    drop(registry);
    let registry = Registry::open(registry_deps(
        store,
        runtime,
        b"new-factory",
        configuration(),
    ))
    .await
    .unwrap();
    let upgraded = registry.status().await.plugins.remove(0).active.unwrap();
    assert_eq!(upgraded.artifact, ContentHash::of(b"new-factory"));
    assert_eq!(
        upgraded.settings.config,
        serde_json::json!({"label":"good"})
    );
    let factory = registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), true)
        .unwrap()
        .generation;
    assert!(upgraded.id < factory);
    registry
        .report(
            upgraded.id,
            Verdict::Failed(super::fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.plugins.remove(0).active.unwrap().id,
        good
    );
}
