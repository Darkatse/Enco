//! How verdicts move a generation through trial to health or rollback, and what survives restart.
use super::{active_activation, fault, loaded, open, promote, register_source};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{path::Path, sync::Arc};

async fn factory_only(root: &Path) -> (Arc<SqliteStore>, Arc<ScriptedRuntime>) {
    let store = Arc::new(open_store(root).await);
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    (store, runtime)
}

#[tokio::test]
async fn trial_promotion_counts_successes_since_opening_and_ignores_environment_failures() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime.clone()).await;
    let trial = register_source(&registry, "custom", FACTORY_BYTES).await;
    for _ in 0..TRIAL_CALLS - 1 {
        registry
            .report(&trial.id.into(), Verdict::Ok, None)
            .await
            .unwrap();
    }
    drop(registry);
    let registry = open(&store, runtime).await;
    for _ in 0..TRIAL_CALLS - 1 {
        registry
            .report(&trial.id.into(), Verdict::Ok, None)
            .await
            .unwrap();
    }
    for code in [
        code::PROVIDER_NETWORK,
        code::TIMEOUT,
        code::CANCELLED,
        code::PLUGIN_UNAVAILABLE,
    ] {
        registry
            .report(&trial.id.into(), Verdict::Failed(fault(code)), None)
            .await
            .unwrap();
    }
    assert_eq!(
        store.registry().await.unwrap().node.health[&trial.id],
        Health::Trial
    );
    registry
        .report(&trial.id.into(), Verdict::Ok, None)
        .await
        .unwrap();
    assert_eq!(
        store.registry().await.unwrap().node.health[&trial.id],
        Health::Healthy
    );
    // A promoted generation is no longer rolled back by its own failures.
    registry
        .report(
            &trial.id.into(),
            Verdict::Failed(fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .code,
        CodeRef::from(trial.id)
    );
}

#[tokio::test]
async fn rollback_commits_with_its_notice_and_both_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime.clone()).await;
    let healthy = register_source(&registry, "custom", FACTORY_BYTES).await;
    promote(&registry, healthy.id).await;
    let next = register_source(&registry, "custom", FACTORY_BYTES).await;
    let session = store
        .ensure_session(&"notice".parse().unwrap(), Utc::now())
        .await
        .unwrap();
    let sql = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON inbox BEGIN SELECT RAISE(ABORT, 'notice unavailable'); END;").unwrap();
    assert!(
        registry
            .report(
                &next.id.into(),
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
            .code,
        CodeRef::from(next.id)
    );
    let history = store.registry().await.unwrap();
    assert_eq!(history.intent.generations.values().last().unwrap(), &next);
    assert_eq!(history.node.health[&next.id], Health::Trial);
    assert!(store.pending(session.id).await.unwrap().is_empty());
    sql.execute_batch("DROP TRIGGER reject_notice").unwrap();
    let intent = super::intent_tables(dir.path());
    // The second report concerns a generation that is no longer active, so it changes nothing.
    for _ in 0..2 {
        registry
            .report(
                &next.id.into(),
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
            .code,
        CodeRef::from(healthy.id)
    );
    // Recovery moves this node's references and leaves what was registered as it was.
    assert_eq!(super::intent_tables(dir.path()), intent);
    let rollback = RolledBack {
        plugin: "custom".parse().unwrap(),
        from: next.id,
        to: Some(CodeRef::from(healthy.id).code),
        failure: fault(code::PLUGIN_TRAP),
    };
    drop(registry);
    drop(store);
    let store = Arc::new(open_store(dir.path()).await);
    let registry = open(&store, runtime).await;
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("custom"), false)
            .unwrap()
            .code,
        CodeRef::from(healthy.id)
    );
    let intent = registry.registered_intent(None).await.unwrap();
    let failed = &intent.plugins["custom"].activation;
    assert_eq!(
        failed.health,
        Some(Health::Failed {
            failure: rollback.failure.clone()
        })
    );
    let pending = store.pending(session.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert!(
        matches!(&pending[0].body, EventBody::PluginRolledBack { rollback: recorded, .. } if *recorded == rollback)
    );
}

#[tokio::test]
async fn a_failing_only_generation_leaves_its_plugin_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let (store, runtime) = factory_only(dir.path()).await;
    let registry = open(&store, runtime).await;
    let alone = register_source(&registry, "alone", FACTORY_BYTES).await;
    registry
        .report(
            &alone.id.into(),
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
    let store = Arc::new(open_store(dir.path()).await);
    let runtime = Arc::new(ScriptedRuntime::default());
    let provider = ScriptedProvider::new(vec![reply("OK"), Err(fault(code::PLUGIN_TRAP))]);
    runtime.insert(FACTORY_BYTES, loaded(provider));
    let registry = open(&store, runtime).await;
    let trial = submit(&registry, source_request("fixture", FACTORY_BYTES))
        .await
        .unwrap()
        .generations
        .remove(0)
        .id;
    let mut document = configuration();
    document["endpoint"]["reply"]["model"] = "probe-success".into();
    submit(&registry, configuration_request(document.clone()))
        .await
        .unwrap();
    for _ in 0..TRIAL_CALLS - 2 {
        registry
            .report(&trial.into(), Verdict::Ok, None)
            .await
            .unwrap();
    }
    assert_eq!(
        active_activation(&registry).await.health,
        Some(Health::Trial)
    );
    registry
        .report(&trial.into(), Verdict::Ok, None)
        .await
        .unwrap();
    assert_eq!(
        active_activation(&registry).await.health,
        Some(Health::Healthy)
    );
    let broken = submit(&registry, source_request("fixture", FACTORY_BYTES))
        .await
        .unwrap()
        .generations
        .remove(0)
        .id;
    let config = registry.view().config.registration;
    document["endpoint"]["reply"]["model"] = "probe-trap".into();
    let result = submit(&registry, configuration_request(document)).await;
    assert!(matches!(result, Err(RegistryError::Rejected { .. })));
    assert_eq!(registry.view().config.registration, config);
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("fixture"), false)
            .unwrap()
            .code,
        CodeRef::from(trial)
    );
    let history = store.registry().await.unwrap();
    assert!(matches!(
        history.node.health[&broken],
        Health::Failed { .. }
    ));
    assert_eq!(
        registry.registered_intent(None).await.unwrap().plugins["fixture"]
            .activation
            .reference
            .code(),
        CodeRef::from(broken).code
    );
}

#[tokio::test]
async fn changing_only_image_acceptance_probes_before_registering_configuration() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        Err(fault(code::PROVIDER_BAD_REQUEST)),
        reply("image accepted"),
    ]);
    let (registry, _) = registry_configured(
        root.path(),
        loaded(provider.clone()),
        configuration(),
        vec![],
    )
    .await;
    assert!(provider.requests.lock().unwrap().is_empty());
    let before = registry.view().config.document.clone();
    let mut document = before.clone();
    document["endpoint"]["reply"]["accepts"] = serde_json::json!(["image"]);
    let rejected = submit(&registry, configuration_request(document.clone())).await;
    assert!(matches!(rejected, Err(RegistryError::Rejected(_))));
    assert_eq!(registry.view().config.document, before);
    submit(&registry, configuration_request(document.clone()))
        .await
        .unwrap();
    assert_eq!(registry.view().config.document, document);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        let images: Vec<_> = request
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .filter_map(|part| match part {
                Part::Image(image) => Some(image),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].format, ImageFormat::Png);
        assert_eq!(
            images[0].data,
            include_bytes!("../../../../enco-kernel/src/image/probe.png")
        );
    }
}
