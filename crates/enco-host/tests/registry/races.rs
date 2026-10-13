//! Which change wins when another commit lands while a generation is still loading.
use super::{deploy_artifact, fault, loaded, open, promote};
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
    entered: Notify,
    resume: Notify,
}

#[async_trait]
impl Runtime for PausedLoad {
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError> {
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
async fn rollback_cannot_overwrite_a_deployment_committed_during_loading() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let scripted = Arc::new(ScriptedRuntime::default());
    for bytes in [FACTORY_BYTES, b"one", b"two", b"three"] {
        scripted.insert(bytes, loaded(ScriptedProvider::new(vec![])));
    }
    let runtime = Arc::new(PausedLoad {
        runtime: scripted,
        pause: Mutex::new(None),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let registry = open(&store, runtime.clone()).await;
    let one = deploy_artifact(&registry, "fixture", b"one").await;
    promote(&registry, one.id).await;
    let two = deploy_artifact(&registry, "fixture", b"two").await;
    promote(&registry, two.id).await;
    *runtime.pause.lock().unwrap() = Some(one.artifact);
    let rollback = tokio::spawn({
        let registry = registry.clone();
        async move { registry.rollback("fixture").await }
    });
    runtime.entered.notified().await;
    let three = deploy_artifact(&registry, "fixture", b"three").await;
    runtime.resume.notify_one();
    assert!(matches!(
        rollback.await.unwrap(),
        Err(RegistryError::Conflict(_))
    ));
    let status = registry.status().await.plugins.remove(0);
    assert_eq!(status.generations.len(), 4);
    assert_eq!(status.active.unwrap().id, three.id);
    let previous = registry.rollback("fixture").await.unwrap();
    assert_eq!(previous.id, two.id);
}

#[tokio::test]
async fn automatic_rollback_discards_preparation_after_deployment_or_promotion() {
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
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let registry = open(&store, runtime.clone()).await;
        let healthy = deploy_artifact(&registry, "fixture", b"healthy").await;
        promote(&registry, healthy.id).await;
        let trial = deploy_artifact(&registry, "fixture", b"trial").await;
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
                .completion("fixture", false)
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
                .iter()
                .all(|record| record.status != GenerationStatus::Failed && record.failure.is_none())
        );
        assert!(store.pending(session.id).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn deployment_discards_preparation_when_its_plugin_or_configuration_changes() {
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
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let registry = open(&store, runtime.clone()).await;
        let deploy = registry.deploy(
            "fixture",
            enco_kernel::PluginSource::Artifact(b"late".to_vec()),
        );
        let change = async {
            runtime.entered.notified().await;
            if change_config {
                let mut document = configuration();
                document["run"] = serde_json::json!({"max_rounds": 7});
                registry
                    .apply_config(ConfigSource::Document(document))
                    .await
                    .unwrap();
            } else {
                deploy_artifact(&registry, "fixture", b"winner").await;
            }
            runtime.resume.notify_one();
        };
        let (result, ()) = tokio::join!(deploy, change);
        assert!(matches!(result, Err(RegistryError::Conflict(_))));
        let status = registry.status().await.plugins.remove(0);
        assert!(
            status
                .generations
                .iter()
                .all(|record| record.artifact != ContentHash::of(b"late"))
        );
        // The first committer stays active.
        let first: &[u8] = if change_config {
            FACTORY_BYTES
        } else {
            b"winner"
        };
        assert_eq!(status.active.unwrap().artifact, ContentHash::of(first));
    }
}
