//! How verdicts move a generation through trial to health or rollback, and what survives restart.
use super::{deploy_artifact, fault, loaded, open, promote};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{path::Path, sync::Arc};

async fn factory_only(root: &Path) -> (Arc<SqliteStore>, Arc<ScriptedRuntime>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    (store, runtime)
}

#[tokio::test]
async fn trial_promotion_counts_successes_since_opening_and_ignores_environment_failures() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime.clone()).await;
    let trial = deploy_artifact(&registry, "custom", FACTORY_BYTES).await;
    for _ in 0..TRIAL_CALLS - 1 {
        registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    }
    drop(registry);
    let registry = open(&store, runtime).await;
    for _ in 0..TRIAL_CALLS - 1 {
        registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    }
    for code in [
        code::PROVIDER_NETWORK,
        code::TIMEOUT,
        code::CANCELLED,
        code::PLUGIN_UNAVAILABLE,
    ] {
        registry
            .report(trial.id, Verdict::Failed(fault(code)), None)
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .registry()
            .await
            .unwrap()
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Trial
    );
    registry.report(trial.id, Verdict::Ok, None).await.unwrap();
    assert_eq!(
        store
            .registry()
            .await
            .unwrap()
            .generations
            .last()
            .unwrap()
            .status,
        GenerationStatus::Healthy
    );
    // A promoted generation is no longer rolled back by its own failures.
    registry
        .report(trial.id, Verdict::Failed(fault(code::PLUGIN_TRAP)), None)
        .await
        .unwrap();
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .generation,
        trial.id
    );
}

#[tokio::test]
async fn rollback_commits_with_its_notice_and_both_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime.clone()).await;
    let healthy = deploy_artifact(&registry, "custom", FACTORY_BYTES).await;
    promote(&registry, healthy.id).await;
    let next = deploy_artifact(&registry, "custom", FACTORY_BYTES).await;
    let session = store.ensure_session("notice", Utc::now()).await.unwrap();
    let sql = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON inbox BEGIN SELECT RAISE(ABORT, 'notice unavailable'); END;").unwrap();
    assert!(
        registry
            .report(
                next.id,
                Verdict::Failed(fault(code::PLUGIN_TRAP)),
                Some(session.id)
            )
            .await
            .is_err()
    );
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .generation,
        next.id
    );
    assert_eq!(
        store.registry().await.unwrap().generations.last().unwrap(),
        &next
    );
    assert!(store.pending(session.id).await.unwrap().is_empty());
    sql.execute_batch("DROP TRIGGER reject_notice").unwrap();
    // The second report concerns a generation that is no longer active, so it changes nothing.
    for _ in 0..2 {
        registry
            .report(
                next.id,
                Verdict::Failed(fault(code::PLUGIN_TRAP)),
                Some(session.id),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .generation,
        healthy.id
    );
    let rollback = RolledBack {
        plugin: "custom".into(),
        from: next.id,
        to: Some(healthy.id),
        failure: fault(code::PLUGIN_TRAP),
    };
    drop(registry);
    drop(store);
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let registry = open(&store, runtime).await;
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .generation,
        healthy.id
    );
    let status = registry.status().await.plugins;
    let failed = status
        .iter()
        .find(|plugin| plugin.name == "custom")
        .unwrap()
        .generations
        .last()
        .unwrap();
    assert_eq!(failed.status, GenerationStatus::Failed);
    assert_eq!(failed.failure.as_ref(), Some(&rollback.failure));
    let pending = store.pending(session.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert!(
        matches!(&pending[0].body, EventBody::GenerationRolledBack { rollback: recorded, .. } if *recorded == rollback)
    );
}

#[tokio::test]
async fn a_failing_only_generation_leaves_its_plugin_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime).await;
    let alone = deploy_artifact(&registry, "alone", FACTORY_BYTES).await;
    registry
        .report(
            alone.id,
            Verdict::Failed(fault(code::PLUGIN_CONTRACT)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("alone"), false)
            .err()
            .unwrap()
            .code,
        code::PLUGIN_UNAVAILABLE
    );
}

#[tokio::test]
async fn configuration_probes_report_trial_health_even_when_configuration_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![reply("OK"), Err(fault(code::PLUGIN_TRAP))]);
    runtime.insert(FACTORY_BYTES, loaded(provider));
    let registry = open(&store, runtime).await;
    let factory = registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), true)
        .unwrap()
        .generation;
    let trial = registry
        .deploy("fixture", PluginSource::Generation(factory))
        .await
        .unwrap()
        .generation
        .id;
    let mut document = configuration();
    document["endpoint"]["reply"]["model"] = "probe-success".into();
    registry
        .apply_config(ConfigSource::Document(document.clone()))
        .await
        .unwrap();
    for _ in 0..TRIAL_CALLS - 2 {
        registry.report(trial, Verdict::Ok, None).await.unwrap();
    }
    assert_eq!(
        registry.status().await.plugins[0]
            .active
            .as_ref()
            .unwrap()
            .status,
        GenerationStatus::Trial
    );
    registry.report(trial, Verdict::Ok, None).await.unwrap();
    assert_eq!(
        registry.status().await.plugins[0]
            .active
            .as_ref()
            .unwrap()
            .status,
        GenerationStatus::Healthy
    );
    let broken = registry
        .deploy("fixture", PluginSource::Generation(factory))
        .await
        .unwrap()
        .generation
        .id;
    let config = registry.view().config.generation;
    document["endpoint"]["reply"]["model"] = "probe-trap".into();
    let result = registry
        .apply_config(ConfigSource::Document(document))
        .await;
    assert!(matches!(result, Err(RegistryError::Rejected { .. })));
    assert_eq!(registry.view().config.generation, config);
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("fixture"), false)
            .unwrap()
            .generation,
        trial
    );
    let status = registry.status().await;
    assert_eq!(
        status.plugins[0]
            .generations
            .iter()
            .find(|g| g.id == broken)
            .unwrap()
            .status,
        GenerationStatus::Failed
    );
}
