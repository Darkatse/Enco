use super::promote;
use crate::support::*;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::*;
use std::{path::Path, sync::Arc};

async fn open(
    root: &Path,
    runtime: Arc<dyn Runtime>,
    bytes: &[u8],
    document: serde_json::Value,
) -> (Arc<SqliteStore>, Arc<Registry>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let registry = open_registry(
        registry_deps(store.clone(), runtime, bytes),
        configuration_request(document),
    )
    .await
    .unwrap();
    (store, registry)
}

fn runtime(artifacts: &[&[u8]]) -> Arc<ScriptedRuntime> {
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in artifacts {
        let provider = ScriptedProvider::new(vec![reply(&String::from_utf8_lossy(bytes))]);
        runtime.insert(bytes, fixture(provider.clone(), provider));
    }
    runtime
}

#[tokio::test]
async fn repeated_factory_activation_does_not_make_an_undone_registration_effective_again() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"custom"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let factory = registry.status().await.unwrap().plugins[0]
        .active
        .as_ref()
        .unwrap()
        .id;
    let mut stale = source_request("fixture", b"custom");
    stale.base = registry
        .status()
        .await
        .unwrap()
        .registrations
        .last()
        .map(|r| r.id);
    let custom = submit(&registry, source_request("fixture", b"custom"))
        .await
        .unwrap();
    let custom_generation = custom.generations[0].id;
    promote(&registry, custom_generation).await;
    let restored = submit(
        &registry,
        RegistrationRequest {
            revision: "restore-factory".into(),
            plugins: [("fixture".into(), PluginChange::Factory)].into(),
            base: None,
            configuration: None,
        },
    )
    .await
    .unwrap();
    assert!(restored.generations.is_empty());
    assert!(matches!(
        registry.register(stale).await,
        Err(RegistryError::Conflict(_))
    ));
    assert_eq!(
        registry.rollback().await.unwrap().registration,
        restored.registration.id
    );
    assert_eq!(
        registry.rollback().await.unwrap().registration,
        custom.registration.id
    );
    assert!(matches!(
        registry.rollback().await,
        Err(RegistryError::NoRollbackTarget(_))
    ));
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .id,
        factory
    );
}

#[tokio::test]
async fn rename_round_trip_rolls_back_by_name_version() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"custom"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let created = submit(&registry, source_request("a", b"custom"))
        .await
        .unwrap();
    let identity = created.generations[0].plugin;
    for (name, to) in [("a", "b"), ("b", "a")] {
        submit(
            &registry,
            RegistrationRequest {
                revision: format!("{name}-{to}"),
                plugins: [(name.into(), PluginChange::Rename(to.into()))].into(),
                base: None,
                configuration: None,
            },
        )
        .await
        .unwrap();
    }
    for expected in [Some("b"), Some("a"), None] {
        registry.rollback().await.unwrap();
        let status = registry.status().await.unwrap();
        let plugin = status
            .plugins
            .iter()
            .find(|plugin| plugin.id == identity)
            .unwrap();
        assert_eq!(plugin.name.as_deref(), expected);
    }
    assert!(matches!(
        registry.rollback().await,
        Err(RegistryError::NoRollbackTarget(_))
    ));
}

#[tokio::test]
async fn retained_files_are_bounded_and_a_pinned_adapter_outlives_its_file() {
    let root = tempfile::tempdir().unwrap();
    let versions: Vec<_> = (2..=10)
        .map(|n| format!("version-{n}").into_bytes())
        .collect();
    let artifacts: Vec<&[u8]> = [FACTORY_BYTES, b"one"]
        .into_iter()
        .chain(versions.iter().map(Vec::as_slice))
        .collect();
    let runtime = runtime(&artifacts);
    let (store, registry) =
        open(root.path(), runtime.clone(), FACTORY_BYTES, configuration()).await;
    let first = submit(&registry, source_request("fixture", b"one"))
        .await
        .unwrap()
        .generations
        .remove(0);
    promote(&registry, first.id).await;
    let pinned = registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), false)
        .unwrap();
    for (index, bytes) in versions.iter().enumerate() {
        let generation = submit(&registry, source_request("fixture", bytes))
            .await
            .unwrap()
            .generations
            .remove(0);
        if index % 3 == 1 {
            registry
                .report(
                    generation.id,
                    Verdict::Failed(super::fault(code::PLUGIN_TRAP)),
                    None,
                )
                .await
                .unwrap();
        } else {
            promote(&registry, generation.id).await;
        }
        assert!(
            std::fs::read_dir(root.path().join("artifacts"))
                .unwrap()
                .count()
                <= 2
        );
        let status = registry.status().await.unwrap();
        let plugin = &status.plugins[0];
        assert_eq!(plugin.generations.len(), index + 3);
    }
    assert!(matches!(
        store.artifact(&first.artifact).await,
        Err(StoreError::Artifact(_))
    ));
    let reply = pinned
        .adapter
        .complete(
            &fixture_endpoint("fixture").settings,
            ProviderRequest {
                messages: vec![],
                tools: vec![],
                max_output_tokens: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(reply.message, Message::text(Role::Assistant, "one"));
    let orphan = ContentHash::of(b"orphan");
    std::fs::write(
        root.path().join("artifacts").join(format!("{orphan}.wasm")),
        b"orphan",
    )
    .unwrap();
    drop(registry);
    let (_, reopened) = open(root.path(), runtime, FACTORY_BYTES, configuration()).await;
    assert!(matches!(
        store.artifact(&orphan).await,
        Err(StoreError::Artifact(_))
    ));
    assert_eq!(
        reopened.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"version-10")
    );
}

#[tokio::test]
async fn rejected_durable_registration_does_not_leave_an_unreferenced_artifact() {
    let root = tempfile::tempdir().unwrap();
    let (store, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"one", b"rejected"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    submit(&registry, source_request("fixture", b"one"))
        .await
        .unwrap();
    let before = store.registry().await.unwrap();
    let sql = rusqlite::Connection::open(root.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_registration BEFORE INSERT ON registrations BEGIN SELECT RAISE(FAIL, 'registration unavailable'); END").unwrap();
    assert!(
        submit(&registry, source_request("fixture", b"rejected"))
            .await
            .is_err()
    );
    assert_eq!(
        store.registry().await.unwrap().registrations,
        before.registrations
    );
    assert!(matches!(
        store.artifact(&ContentHash::of(b"rejected")).await,
        Err(StoreError::Artifact(_))
    ));
    assert_eq!(
        std::fs::read_dir(root.path().join("artifacts"))
            .unwrap()
            .count(),
        2
    );
}

#[tokio::test]
async fn owner_rollback_restores_only_the_still_effective_parts_of_the_latest_batch() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"a1", b"b1", b"a2", b"b2", b"other"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let mut first = source_request("a", b"a1");
    first.plugins.extend(source_request("b", b"b1").plugins);
    let first = submit(&registry, first).await.unwrap();
    for generation in &first.generations {
        promote(&registry, generation.id).await;
    }
    let before = registry.status().await.unwrap();
    let mut document = configuration();
    document["owner"]["timezone"] = "America/New_York".into();
    let batch = || {
        let mut request = source_request("a", b"a2");
        request.plugins.extend(source_request("b", b"b2").plugins);
        request.configuration = Some(document.clone());
        request
    };
    let applied = submit(&registry, batch()).await.unwrap();
    let a = first
        .generations
        .iter()
        .find(|generation| generation.artifact == ContentHash::of(b"a1"))
        .unwrap()
        .plugin;
    let broken = applied
        .generations
        .iter()
        .find(|generation| generation.plugin == a)
        .unwrap()
        .id;
    registry
        .report(
            broken,
            Verdict::Failed(Failure {
                code: code::PLUGIN_TRAP.into(),
                message: "broken a".into(),
                retryable: false,
            }),
            None,
        )
        .await
        .unwrap();
    submit(&registry, source_request("other", b"other"))
        .await
        .unwrap();
    registry.rollback().await.unwrap();
    assert_eq!(
        registry.rollback().await.unwrap().registration,
        applied.registration.id
    );
    submit(&registry, source_request("extra", b"other"))
        .await
        .unwrap();
    let after = registry.status().await.unwrap();
    assert_eq!(after.config.active.id, before.config.active.id);
    assert_eq!(after.registered.configuration.unwrap().document, document);
    for old in first.generations {
        let plugin = after
            .plugins
            .iter()
            .find(|plugin| plugin.id == old.plugin)
            .unwrap();
        assert_eq!(plugin.active.as_ref().unwrap().id, old.id);
        assert!(!plugin.matches_registered);
    }
}

#[tokio::test]
async fn rejected_recovery_does_not_leave_staged_factory_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (store, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"one", b"two"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let first = submit(&registry, source_request("fixture", b"one"))
        .await
        .unwrap()
        .generations
        .remove(0);
    promote(&registry, first.id).await;
    let second = submit(&registry, source_request("fixture", b"two"))
        .await
        .unwrap()
        .generations
        .remove(0);
    std::fs::remove_file(
        root.path()
            .join("artifacts")
            .join(format!("{}.wasm", first.artifact)),
    )
    .unwrap();
    let sql = rusqlite::Connection::open(root.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_recovery BEFORE UPDATE ON plugins BEGIN SELECT RAISE(FAIL, 'recovery unavailable'); END").unwrap();
    assert!(registry.rollback().await.is_err());
    assert!(matches!(
        store.artifact(&ContentHash::of(FACTORY_BYTES)).await,
        Err(StoreError::Artifact(_))
    ));
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .id,
        second.id
    );
}

#[tokio::test]
async fn cleanup_failure_reports_accepted_registration_and_rollback() {
    let root = tempfile::tempdir().unwrap();
    let (store, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"one"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let obstruction = root.path().join("artifacts/unreferenced-directory");
    std::fs::create_dir(&obstruction).unwrap();
    let accepted = submit(&registry, source_request("fixture", b"one"))
        .await
        .unwrap();
    assert!(!accepted.warnings.is_empty());
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .id,
        accepted.generations[0].id
    );
    assert_eq!(
        store
            .registry()
            .await
            .unwrap()
            .registrations
            .values()
            .next_back(),
        Some(&accepted.registration)
    );
    let recovered = registry.rollback().await.unwrap();
    assert!(!recovered.warnings.is_empty());
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(FACTORY_BYTES)
    );
    std::fs::remove_dir(obstruction).unwrap();
}

#[tokio::test]
async fn rollback_of_deletion_restores_the_name_and_its_configuration_with_only_healthy_code() {
    for healthy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(ScriptedRuntime::default());
        for bytes in [FACTORY_BYTES, b"custom"] {
            let provider = ScriptedProvider::new(vec![reply("probe"), reply("probe")]);
            runtime.insert(bytes, fixture(provider.clone(), provider));
        }
        let (_, registry) = open(root.path(), runtime, FACTORY_BYTES, configuration()).await;
        let mut custom_config = configuration();
        for endpoint in ["reply", "compaction", "embed"] {
            custom_config["endpoint"][endpoint]["plugin"] = "custom".into();
        }
        let mut request = source_request("custom", b"custom");
        request.configuration = Some(custom_config.clone());
        let added = submit(&registry, request).await.unwrap();
        let code = added.generations[0].clone();
        if healthy {
            promote(&registry, code.id).await;
        }
        let deleted = submit(
            &registry,
            RegistrationRequest {
                revision: "delete-and-rewire".into(),
                plugins: [("custom".into(), PluginChange::Remove)].into(),
                configuration: Some(configuration()),
                base: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            registry.rollback().await.unwrap().registration,
            deleted.registration.id
        );
        let status = registry.status().await.unwrap();
        assert_eq!(status.config.active.document, custom_config);
        let restored = status
            .plugins
            .iter()
            .find(|plugin| plugin.id == code.plugin)
            .unwrap();
        assert_eq!(
            restored.active.as_ref().map(|record| record.id),
            healthy.then_some(code.id)
        );
        assert_eq!(restored.name.as_deref(), healthy.then_some("custom"));
    }
}

#[tokio::test]
async fn rollback_restores_the_configuration_active_before_registration() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let original = registry.status().await.unwrap().config.active;
    let mut document = configuration();
    document["owner"]["timezone"] = "Europe/Paris".into();
    submit(&registry, configuration_request(document.clone()))
        .await
        .unwrap();
    registry.rollback().await.unwrap();
    document["run"] = serde_json::json!({"max_rounds": 30});
    let latest = submit(&registry, configuration_request(document))
        .await
        .unwrap();
    registry.rollback().await.unwrap();
    let status = registry.status().await.unwrap();
    assert_eq!(status.config.active, original);
    assert_eq!(status.registered.configuration, latest.configuration);
}

#[tokio::test]
async fn empty_requests_and_removal_of_reserved_factory_names_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let before = registry.status().await.unwrap().registrations;
    for plugins in [
        Default::default(),
        [("fixture".into(), PluginChange::Remove)].into(),
    ] {
        let request = RegistrationRequest {
            revision: "rejected".into(),
            base: None,
            configuration: None,
            plugins,
        };
        assert!(matches!(
            submit(&registry, request).await,
            Err(RegistryError::Rejected { .. })
        ));
    }
    assert_eq!(registry.status().await.unwrap().registrations, before);
}

#[tokio::test]
async fn new_source_cannot_bind_to_a_name_restored_only_by_rollback() {
    let root = tempfile::tempdir().unwrap();
    let (_, registry) = open(
        root.path(),
        runtime(&[FACTORY_BYTES, b"old", b"new"]),
        FACTORY_BYTES,
        configuration(),
    )
    .await;
    let old = submit(&registry, source_request("a", b"old"))
        .await
        .unwrap()
        .generations
        .remove(0);
    submit(
        &registry,
        RegistrationRequest {
            revision: "a-to-b".into(),
            base: None,
            configuration: None,
            plugins: [("a".into(), PluginChange::Rename("b".into()))].into(),
        },
    )
    .await
    .unwrap();
    registry.rollback().await.unwrap();
    let before = registry.status().await.unwrap().registrations;
    let result = submit(&registry, source_request("a", b"new")).await;
    assert!(matches!(result, Err(RegistryError::Rejected { .. })));
    let status = registry.status().await.unwrap();
    assert_eq!(status.registrations, before);
    assert_eq!(status.registered.plugins["b"].id, old.plugin);
    let running = status
        .plugins
        .iter()
        .find(|plugin| plugin.id == old.plugin)
        .unwrap();
    assert_eq!(running.name.as_deref(), Some("a"));
    assert_eq!(running.active.as_ref().unwrap().id, old.id);
}
