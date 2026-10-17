//! Which change wins when another commit lands while a generation is still loading.
use super::{active, deploy_artifact, fault, loaded, open, promote};
use crate::support::*;
use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

struct PausedLoad {
    runtime: Arc<ScriptedRuntime>,
    pause: Mutex<Option<ContentHash>>,
    unavailable: Mutex<Option<ContentHash>>,
    entered: Notify,
    resume: Notify,
}

#[async_trait]
impl Runtime for PausedLoad {
    async fn load(&self, artifact: &[u8], config: &PluginSettings) -> Result<Loaded, LoadError> {
        if self.unavailable.lock().unwrap().as_ref() == Some(&ContentHash::of(artifact)) {
            return Err(LoadError::Component(
                "temporarily unavailable adapter".into(),
            ));
        }
        let result = self.runtime.load(artifact, config).await;
        let pause = self
            .pause
            .lock()
            .unwrap()
            .take_if(|hash| *hash == ContentHash::of(artifact))
            .is_some();
        if pause {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        result
    }
}

#[tokio::test]
async fn registration_cannot_overwrite_a_rollback_committed_during_loading() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let scripted = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"one", b"two", b"three"] {
        scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let runtime = Arc::new(PausedLoad {
        runtime: scripted,
        pause: Mutex::new(None),
        unavailable: Mutex::new(None),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let registry = open(&store, runtime.clone()).await;
    let one = deploy_artifact(&registry, "fixture", b"one").await;
    promote(&registry, one.id).await;
    let two = deploy_artifact(&registry, "fixture", b"two").await;
    promote(&registry, two.id).await;
    *runtime.pause.lock().unwrap() = Some(ContentHash::of(b"three"));
    let deploy = submit(&registry, artifact_request("fixture", b"three".to_vec()));
    let rollback = async {
        runtime.entered.notified().await;
        registry.rollback().await.unwrap();
        runtime.resume.notify_one();
    };
    let (result, ()) = tokio::join!(deploy, rollback);
    assert!(matches!(result, Err(RegistryError::Conflict(_))));
    assert_eq!(store.registry().await.unwrap().generations.len(), 3);
    assert_eq!(active(&registry).await.id, one.id);
}

#[tokio::test]
async fn a_late_failure_after_replacement_does_not_roll_back_or_publish_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"old", b"new"] {
        runtime.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let registry = open(&store, runtime).await;
    let old = deploy_artifact(&registry, "fixture", b"old").await;
    let new = deploy_artifact(&registry, "fixture", b"new").await;
    let session = store.ensure_session("notice", Utc::now()).await.unwrap();
    registry
        .report(
            old.id,
            Verdict::Failed(fault(code::PLUGIN_TRAP)),
            Some(session.id),
        )
        .await
        .unwrap();
    let history = store.registry().await.unwrap();
    assert_eq!(
        history.plugins[&old.plugin].active.unwrap().generation,
        new.id
    );
    assert_eq!(history.generations[&old.id].status, GenerationStatus::Trial);
    assert!(store.pending(session.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn registration_discards_preparation_when_its_plugin_or_configuration_changes() {
    for change_config in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
        let scripted = Arc::new(ScriptedRuntime::default());
        for bytes in [FACTORY_BYTES, b"late", b"winner"] {
            scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
        }
        let runtime = Arc::new(PausedLoad {
            runtime: scripted,
            pause: Mutex::new(Some(ContentHash::of(b"late"))),
            unavailable: Mutex::new(None),
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let registry = open(&store, runtime.clone()).await;
        let deploy = submit(&registry, artifact_request("fixture", b"late".to_vec()));
        let change = async {
            runtime.entered.notified().await;
            if change_config {
                let mut document = configuration();
                document["run"] = serde_json::json!({"max_rounds": 7});
                submit(&registry, configuration_request(document))
                    .await
                    .unwrap();
            } else {
                deploy_artifact(&registry, "fixture", b"winner").await;
            }
            runtime.resume.notify_one();
        };
        let (result, ()) = tokio::join!(deploy, change);
        assert!(matches!(result, Err(RegistryError::Conflict(_))));
        assert!(
            store
                .registry()
                .await
                .unwrap()
                .generations
                .values()
                .all(|record| record.artifact != ContentHash::of(b"late"))
        );
        let first: &[u8] = if change_config {
            FACTORY_BYTES
        } else {
            b"winner"
        };
        assert_eq!(active(&registry).await.artifact, ContentHash::of(first));
    }
}

#[tokio::test]
async fn rollback_cannot_overwrite_a_registration_committed_during_loading() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let scripted = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"one", b"two", b"three"] {
        scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let runtime = Arc::new(PausedLoad {
        runtime: scripted,
        pause: Mutex::new(None),
        unavailable: Mutex::new(None),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let registry = open(&store, runtime.clone()).await;
    let one = deploy_artifact(&registry, "fixture", b"one").await;
    promote(&registry, one.id).await;
    let two = deploy_artifact(&registry, "fixture", b"two").await;
    promote(&registry, two.id).await;
    drop(registry);
    let registry = reopen_without_adapter(&store, &runtime, one.artifact).await;
    *runtime.pause.lock().unwrap() = Some(one.artifact);
    let rollback = tokio::spawn({
        let registry = registry.clone();
        async move { registry.rollback().await }
    });
    runtime.entered.notified().await;
    let three = deploy_artifact(&registry, "fixture", b"three").await;
    runtime.resume.notify_one();
    assert!(matches!(
        rollback.await.unwrap(),
        Err(RegistryError::Conflict(_))
    ));
    assert_eq!(store.registry().await.unwrap().generations.len(), 4);
    assert_eq!(active(&registry).await.id, three.id);
    registry.rollback().await.unwrap();
    assert_eq!(active(&registry).await.id, two.id);
}

#[tokio::test]
async fn automatic_rollback_discards_preparation_after_registration_or_promotion() {
    for promote_current in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
        let scripted = Arc::new(ScriptedRuntime::default());
        for bytes in [FACTORY_BYTES, b"healthy", b"trial"] {
            scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
        }
        let runtime = Arc::new(PausedLoad {
            runtime: scripted,
            pause: Mutex::new(None),
            unavailable: Mutex::new(None),
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let registry = open(&store, runtime.clone()).await;
        let healthy = deploy_artifact(&registry, "fixture", b"healthy").await;
        promote(&registry, healthy.id).await;
        let trial = deploy_artifact(&registry, "fixture", b"trial").await;
        drop(registry);
        let registry = reopen_without_adapter(&store, &runtime, healthy.artifact).await;
        *runtime.pause.lock().unwrap() = Some(healthy.artifact);
        let session = store.ensure_session("notice", Utc::now()).await.unwrap();
        let report = tokio::spawn({
            let registry = registry.clone();
            async move {
                registry
                    .report(
                        trial.id,
                        Verdict::Failed(fault(code::PLUGIN_TRAP)),
                        Some(session.id),
                    )
                    .await
            }
        });
        runtime.entered.notified().await;
        let expected = if promote_current {
            promote(&registry, trial.id).await;
            trial.id
        } else {
            deploy_artifact(&registry, "fixture", b"trial").await.id
        };
        runtime.resume.notify_one();
        report.await.unwrap().unwrap();
        assert_eq!(
            registry
                .view()
                .exports
                .completion(&fixture_endpoint("fixture"), false)
                .unwrap()
                .generation,
            expected
        );
        assert!(
            store
                .registry()
                .await
                .unwrap()
                .generations
                .values()
                .all(|record| record.status != GenerationStatus::Failed && record.failure.is_none())
        );
        assert!(store.pending(session.id).await.unwrap().is_empty());
    }
}

/// Retained bytes may outlive an adapter that could not be loaded at startup.
async fn reopen_without_adapter(
    store: &Arc<SqliteStore>,
    runtime: &Arc<PausedLoad>,
    artifact: ContentHash,
) -> Arc<Registry> {
    *runtime.unavailable.lock().unwrap() = Some(artifact);
    let registry = open(store, runtime.clone()).await;
    *runtime.unavailable.lock().unwrap() = None;
    registry
}
