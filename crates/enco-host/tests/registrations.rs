#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::{SqliteStore, development::Development};
use enco_kernel::*;
use std::{path::Path, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;

async fn open_following(
    root: &Path,
    runtime: Arc<dyn Runtime>,
    bytes: &[u8],
    document: serde_json::Value,
) -> (Arc<SqliteStore>, Arc<Development>, Arc<Registry>) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let (development, registry) = open_home(
        root,
        store.clone(),
        registry_deps(store.clone(), runtime, bytes),
        document,
    )
    .await;
    (store, development, registry)
}

fn runtime(artifacts: &[&[u8]]) -> Arc<ScriptedRuntime> {
    let runtime = Arc::new(ScriptedRuntime::default());
    for bytes in artifacts {
        let provider = ScriptedProvider::new(vec![reply(&String::from_utf8_lossy(bytes))]);
        runtime.insert(bytes, fixture(provider.clone(), provider));
    }
    runtime
}

async fn promote(registry: &Registry, generation: GenerationId) {
    for _ in 0..TRIAL_CALLS {
        registry
            .report(generation, Verdict::Ok, None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn default_factory_follow_activates_existing_host_code_and_rollback_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(&[b"one", b"two", b"three"]);
    let (_, _, first) = open_following(root.path(), runtime.clone(), b"one", configuration()).await;
    let old = first
        .status()
        .await
        .unwrap()
        .plugins
        .remove(0)
        .active
        .unwrap();
    drop(first);
    let (_, _, registry) =
        open_following(root.path(), runtime.clone(), b"two", configuration()).await;
    let status = registry.status().await.unwrap();
    let plugin = &status.plugins[0];
    let active = plugin.active.as_ref().unwrap();
    assert_eq!(active.id, plugin.factory.as_ref().unwrap().generation);
    assert_eq!(active.origin.registration(), None);
    let followed = status.registrations.last().unwrap();
    assert_eq!(
        git(
            root.path(),
            &["rev-parse", &format!("{}^{{tree}}", followed.revision)]
        )
        .await,
        git(
            root.path(),
            &[
                "rev-parse",
                &format!("{}^{{tree}}", status.registrations[0].revision)
            ]
        )
        .await
    );
    assert_eq!(plugin.activated_by, Some(followed.id));
    assert_eq!(followed.code[&plugin.id].generation, active.id);
    assert_eq!(registry.rollback().await.unwrap().registration, followed.id);
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .id,
        old.id
    );
    drop(registry);
    let (_, _, registry) =
        open_following(root.path(), runtime.clone(), b"two", configuration()).await;
    let status = registry.status().await.unwrap();
    assert_eq!(status.registrations.len(), 2);
    assert_eq!(status.plugins[0].active.as_ref().unwrap().id, old.id);
    assert!(!status.plugins[0].matches_registered);
    drop(registry);
    let (_, _, registry) = open_following(root.path(), runtime, b"three", configuration()).await;
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"three")
    );
}

#[tokio::test]
async fn startup_repairs_the_anchor_and_recovers_without_the_repository() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(&[b"one", b"two", b"three"]);
    let (_, _, first) = open_following(root.path(), runtime.clone(), b"one", configuration()).await;
    let previous = first.status().await.unwrap().registrations[0]
        .revision
        .clone();
    drop(first);
    let (store, development, registry) =
        open_following(root.path(), runtime.clone(), b"two", configuration()).await;
    let accepted = registry.status().await.unwrap().registrations[1]
        .revision
        .clone();
    drop(registry);
    git(
        root.path(),
        &["update-ref", "refs/enco/registered", &previous],
    )
    .await;
    let registry = development
        .open(registry_deps(store.clone(), runtime.clone(), b"two"), "two")
        .await
        .unwrap();
    let (_, workspace) = development.status(&registry).await.unwrap();
    assert_eq!(workspace.reference.as_ref(), Some(&accepted));
    drop(registry);
    std::fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    let registry = development
        .open(registry_deps(store.clone(), runtime, b"three"), "three")
        .await
        .unwrap();
    let (status, workspace) = development.status(&registry).await.unwrap();
    assert_eq!(status.registrations.len(), 2);
    assert_eq!(
        status.plugins[0].active.as_ref().unwrap().artifact,
        ContentHash::of(b"two")
    );
    assert!(workspace.unavailable.is_some());
    assert!(!workspace.follow_rejections.is_empty());
    registry.rollback().await.unwrap();
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"one")
    );
    std::fs::rename(root.path().join("saved-git"), root.path().join(".git")).unwrap();
    development
        .apply(&registry, &CancellationToken::new())
        .await
        .unwrap();
    let (status, workspace) = development.status(&registry).await.unwrap();
    assert_eq!(
        status.plugins[0].active.as_ref().unwrap().artifact,
        ContentHash::of(b"three")
    );
    assert!(workspace.follow_rejections.is_empty());
}

struct ConfiguredFactory;

#[async_trait]
impl Runtime for ConfiguredFactory {
    async fn load(&self, bytes: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError> {
        if bytes == b"new"
            && settings
                .config
                .get("label")
                .is_some_and(|label| label != "new")
        {
            return Err(LoadError("new code requires label=new".into()));
        }
        let provider = ScriptedProvider::new(vec![]);
        Ok(fixture(provider.clone(), provider))
    }
}

#[tokio::test]
async fn rejected_follow_keeps_running_state_and_a_configuration_repair_accepts_it() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(ConfiguredFactory);
    let mut document = configuration();
    document["plugin"] = serde_json::json!({"fixture": {"config": {"label": "old"}}});
    let (_, _, first) =
        open_following(root.path(), runtime.clone(), b"old", document.clone()).await;
    let old = first.status().await.unwrap().plugins[0]
        .active
        .as_ref()
        .unwrap()
        .id;
    promote(&first, old).await;
    drop(first);
    let (_, development, registry) =
        open_following(root.path(), runtime.clone(), b"new", document.clone()).await;
    let status = registry.status().await.unwrap();
    let plugin = &status.plugins[0];
    assert_eq!(status.registrations.len(), 1);
    assert_eq!(plugin.active.as_ref().unwrap().id, old);
    assert!(
        development
            .status(&registry)
            .await
            .unwrap()
            .1
            .follow_rejections["fixture"]
            .contains("label=new")
    );
    let factory = plugin.factory.as_ref().unwrap();
    assert_eq!(factory.artifact, ContentHash::of(b"new"));
    assert_eq!(
        registry
            .view()
            .exports
            .completion(&fixture_endpoint("fixture"), true)
            .unwrap()
            .generation,
        factory.generation
    );
    document["plugin"]["fixture"]["config"]["label"] = "new".into();
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let applied = development
        .apply(&registry, &CancellationToken::new())
        .await
        .unwrap();
    assert!(applied.registered.unwrap().configuration.is_some());
    let plugin = registry.status().await.unwrap().plugins.remove(0);
    assert_eq!(
        plugin.active.as_ref().unwrap().artifact,
        ContentHash::of(b"new")
    );
    assert_eq!(
        plugin.active.as_ref().unwrap().settings.config["label"],
        "new"
    );
    assert!(
        development
            .status(&registry)
            .await
            .unwrap()
            .1
            .follow_rejections
            .is_empty()
    );
    assert!(plugin.matches_registered);
}

#[tokio::test]
async fn startup_follows_registered_factory_intent_independently_of_working_directories() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(&[b"one", b"two", b"three"]);
    let (store, development, registry) =
        open_following(root.path(), runtime.clone(), b"one", configuration()).await;
    drop(registry);
    // An unregistered, unbuildable override must not prevent a host upgrade from following intent.
    std::fs::create_dir(root.path().join("plugins/fixture")).unwrap();
    std::fs::write(root.path().join("plugins/fixture/Cargo.toml"), "[broken").unwrap();
    let registry = development
        .open(registry_deps(store.clone(), runtime.clone(), b"two"), "two")
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"two")
    );
    // Byte-identical source still changes intent and stops host following.
    let custom = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"two",
    )
    .await
    .generations
    .remove(0);
    assert!(matches!(custom.origin, Origin::Code { .. }));
    assert_eq!(
        registry.registered_intent().await.plugins["fixture"]
            .code
            .as_ref()
            .map(|code| code.kind),
        Some(CodeIntent::Directory)
    );
    drop(registry);
    std::fs::remove_dir_all(root.path().join("plugins/fixture")).unwrap();
    let registry = development
        .open(registry_deps(store.clone(), runtime, b"three"), "three")
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .id,
        custom.id
    );
    development
        .apply(&registry, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        registry.status().await.unwrap().plugins[0]
            .active
            .as_ref()
            .unwrap()
            .artifact,
        ContentHash::of(b"three")
    );
    // Removing an identical override changes intent back to factory as well.
    source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"three",
    )
    .await;
    std::fs::remove_dir_all(root.path().join("plugins/fixture")).unwrap();
    development
        .apply(&registry, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        registry.registered_intent().await.plugins["fixture"]
            .code
            .as_ref()
            .map(|code| code.kind),
        Some(CodeIntent::Factory)
    );
}

struct PausedRegistration {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[async_trait]
impl Runtime for PausedRegistration {
    async fn load(&self, bytes: &[u8], _settings: &PluginSettings) -> Result<Loaded, LoadError> {
        if bytes == b"paused" {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        let provider = ScriptedProvider::new(vec![]);
        Ok(fixture(provider.clone(), provider))
    }
}

#[tokio::test]
async fn status_remains_available_while_the_registry_loads_a_registration() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(PausedRegistration {
        entered: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
    });
    let (_, development, registry) =
        open_following(root.path(), runtime.clone(), FACTORY_BYTES, configuration()).await;
    let baseline = registry.registered_intent().await.base.unwrap().id;
    let cancel = CancellationToken::new();
    let applying = development.deploy(&registry, "fixture".into(), b"paused".to_vec(), &cancel);
    let inspecting = async {
        runtime.entered.notified().await;
        let (status, workspace) = development.status(&registry).await.unwrap();
        assert_eq!(status.registered.base.unwrap().id, baseline);
        assert!(workspace.unavailable.is_none());
        runtime.resume.notify_one();
    };
    // Status must finish before loading is released; the deadline only bounds a deadlock.
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        tokio::join!(applying, inspecting)
    })
    .await
    .unwrap();
    assert!(result.unwrap().warnings.is_empty());
}

async fn apply_configuration(
    root: &Path,
    development: &Development,
    registry: &Registry,
    document: &serde_json::Value,
) -> enco_host::development::Applied {
    std::fs::write(root.join("config.toml"), toml::to_string(document).unwrap()).unwrap();
    development
        .apply(registry, &CancellationToken::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn code_and_settings_rollbacks_survive_unrelated_registration_until_source_is_fixed() {
    let root = tempfile::tempdir().unwrap();
    let mut document = configuration();
    let (store, development, registry) = open_following(
        root.path(),
        runtime(&[FACTORY_BYTES, b"one", b"two", b"fixed"]),
        FACTORY_BYTES,
        document.clone(),
    )
    .await;
    let running = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"one",
    )
    .await;
    let old = running.generations[0].clone();
    promote(&registry, old.id).await;
    let broken = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"two",
    )
    .await
    .generations
    .remove(0);
    let failure = Failure {
        code: code::PLUGIN_TRAP.into(),
        message: "broken source".into(),
        retryable: false,
    };
    registry
        .report(broken.id, Verdict::Failed(failure.clone()), None)
        .await
        .unwrap();
    document["owner"]["timezone"] = "Europe/Paris".into();
    let unrelated = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert!(unrelated.generations.is_empty() && unrelated.registration.code.is_empty());
    let status = registry.status().await.unwrap();
    let plugin = status
        .plugins
        .iter()
        .find(|plugin| plugin.id == old.plugin)
        .unwrap();
    assert_eq!(plugin.active.as_ref().unwrap().id, old.id);
    assert!(!plugin.matches_registered);
    let intended = &status.registered.plugins["fixture"].code.as_ref().unwrap();
    assert_eq!(intended.generation, broken.id);
    let rejected = plugin
        .generations
        .iter()
        .find(|generation| generation.id == intended.generation)
        .unwrap();
    assert_eq!(rejected.status, GenerationStatus::Failed);
    assert_eq!(rejected.failure.as_ref(), Some(&failure));

    document["plugin"] = serde_json::json!({"fixture": {"config": {"label": "settings"}}});
    let settings = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert_eq!(
        settings.generations[0].origin,
        Origin::Settings {
            registration: settings.registration.id,
            based_on: old.id
        }
    );
    registry.rollback().await.unwrap();
    let plugin = plugin_status(&registry, old.plugin).await;
    assert_eq!(plugin.active.as_ref().unwrap().id, old.id);
    assert_eq!(plugin.active_code, Some(old.id));

    document["run"] = serde_json::json!({"max_rounds": 31});
    let unrelated = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert!(unrelated.generations.is_empty() && unrelated.registration.names.is_empty());
    check_running_source_and_repair(root.path(), &store, &development, &registry, &running).await;
}

/// Status leads from the rolled-back code to the commit that holds its source.
async fn check_running_source_and_repair(
    root: &Path,
    store: &SqliteStore,
    development: &Development,
    registry: &Registry,
    running: &Registered,
) {
    let old = &running.generations[0];
    let status = registry.status().await.unwrap();
    let plugin = status
        .plugins
        .iter()
        .find(|plugin| plugin.id == old.plugin)
        .unwrap();
    assert_eq!(plugin.active.as_ref().unwrap().id, old.id);
    assert!(!plugin.matches_registered);
    assert_eq!(plugin.active_code, Some(old.id));
    let supplier = plugin
        .generations
        .iter()
        .find(|generation| Some(generation.id) == plugin.active_code)
        .unwrap()
        .origin
        .registration()
        .unwrap();
    assert_eq!(supplier, running.registration.id);
    let revision = &status
        .registrations
        .iter()
        .find(|registration| registration.id == supplier)
        .unwrap()
        .revision;
    let source = format!("{revision}:plugins/fixture/source.txt");
    assert_eq!(git(root, &["show", &source]).await, "one");
    let fixed = source_fixture(root, store, development, registry, "fixture", b"fixed")
        .await
        .generations
        .remove(0);
    assert_eq!(
        fixed.settings.config,
        serde_json::json!({"label": "settings"})
    );
    assert!(plugin_status(registry, old.plugin).await.matches_registered);
}

#[tokio::test]
async fn settings_use_registered_names_while_artifacts_replace_running_names() {
    let root = tempfile::tempdir().unwrap();
    let mut document = configuration();
    let (store, development, registry) = open_following(
        root.path(),
        runtime(&[FACTORY_BYTES, b"a", b"artifact", b"c"]),
        FACTORY_BYTES,
        document.clone(),
    )
    .await;
    let old = source_fixture(root.path(), &store, &development, &registry, "a", b"a")
        .await
        .generations
        .remove(0);
    let cancel = CancellationToken::new();
    development
        .rename(&registry, "a", "b", &cancel)
        .await
        .unwrap();
    registry.rollback().await.unwrap();
    let baseline = registry.registered_intent().await.base.unwrap().id;
    document["plugin"] = serde_json::json!({"a": {"config": {"label": "wrong name"}}});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        development.apply(&registry, &cancel).await,
        Err(enco_host::development::DevelopmentError::Registry(
            RegistryError::Rejected { .. }
        ))
    ));
    assert_eq!(
        registry.registered_intent().await.base.unwrap().id,
        baseline
    );
    assert!(matches!(
        registry.check_rename("a", "unused").await,
        Err(RegistryError::UnknownPlugin(_))
    ));

    document["plugin"] = serde_json::json!({"b": {"config": {"label": "registered name"}}});
    let settings = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert_eq!(settings.generations[0].plugin, old.plugin);
    let plugin = plugin_status(&registry, old.plugin).await;
    assert_eq!(plugin.name.as_deref(), Some("a"));
    assert_eq!(
        plugin.active.as_ref().unwrap().settings.config["label"],
        "registered name"
    );
    assert_eq!(plugin.active_code, Some(old.id));
    let artifact = development
        .deploy(&registry, "a".into(), b"artifact".to_vec(), &cancel)
        .await
        .unwrap();
    assert_eq!(artifact.generations[0].plugin, old.plugin);
    source_fixture(root.path(), &store, &development, &registry, "c", b"c").await;
    let collision = development
        .rename(&registry, "c", "a", &cancel)
        .await
        .err()
        .unwrap();
    assert!(collision.to_string().contains("name is already registered"));
}

async fn plugin_status(registry: &Registry, id: PluginId) -> PluginStatus {
    registry
        .status()
        .await
        .unwrap()
        .plugins
        .into_iter()
        .find(|plugin| plugin.id == id)
        .unwrap()
}

#[tokio::test]
async fn unrelated_host_registration_does_not_recreate_a_rolled_back_reused_name() {
    let root = tempfile::tempdir().unwrap();
    let mut document = configuration();
    let (store, development, registry) = open_following(
        root.path(),
        runtime(&[FACTORY_BYTES, b"x", b"y"]),
        FACTORY_BYTES,
        document.clone(),
    )
    .await;
    let x = source_fixture(root.path(), &store, &development, &registry, "custom", b"x")
        .await
        .generations
        .remove(0);
    promote(&registry, x.id).await;
    std::fs::remove_dir_all(root.path().join("plugins/custom")).unwrap();
    development
        .apply(&registry, &CancellationToken::new())
        .await
        .unwrap();
    let y = source_fixture(root.path(), &store, &development, &registry, "custom", b"y")
        .await
        .generations
        .remove(0);
    assert_ne!(x.plugin, y.plugin);
    registry.rollback().await.unwrap();
    registry.rollback().await.unwrap();
    document["owner"]["timezone"] = "Europe/Paris".into();
    let unrelated = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert!(unrelated.generations.is_empty() && unrelated.registration.names.is_empty());
    let restored = plugin_status(&registry, x.plugin).await;
    assert_eq!(restored.active.as_ref().unwrap().id, x.id);
    assert_eq!(restored.name.as_deref(), Some("custom"));
    let removed = plugin_status(&registry, y.plugin).await;
    assert!(removed.active.is_none() && removed.name.is_none());
}
