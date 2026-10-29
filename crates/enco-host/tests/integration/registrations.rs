#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::{
    SqliteStore,
    development::{Development, DevelopmentError},
    status::{FixTarget, Step},
};
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
    let old = running_generation(&first.status().await.unwrap().plugins[0])
        .unwrap()
        .clone();
    drop(first);
    let (_, _, registry) =
        open_following(root.path(), runtime.clone(), b"two", configuration()).await;
    let status = registry.status().await.unwrap();
    let plugin = &status.plugins[0];
    let activation = plugin.running.activation().unwrap();
    let active = &activation.generation;
    assert_eq!(
        active.id,
        status.registered.plugins["fixture"]
            .factory
            .as_ref()
            .unwrap()
            .generation
    );
    assert_eq!(active.origin.registration(), None);
    let followed = registry.history(None, 1).await.unwrap().remove(0);
    assert_eq!(followed.initiator, Initiator::Host);
    assert_eq!(activation.registration, Some(followed.id));
    assert!(followed.changes.iter().any(|change| matches!(change, UnitChange::Plugin { name, change: PluginChangeKind::Factory { generation } } if name.as_ref() == "fixture" && generation.record.id == active.id)));
    assert_eq!(registry.rollback().await.unwrap().registration, followed.id);
    assert_eq!(
        running_generation(&registry.status().await.unwrap().plugins[0])
            .unwrap()
            .id,
        old.id
    );
    drop(registry);
    let (_, _, registry) =
        open_following(root.path(), runtime.clone(), b"two", configuration()).await;
    let status = registry.status().await.unwrap();
    assert_eq!(registry.history(None, 3).await.unwrap().len(), 2);
    assert_eq!(running_generation(&status.plugins[0]).unwrap().id, old.id);
    assert_eq!(
        status.plugins[0].standing,
        Standing::Registered {
            differences: vec![Difference::Code]
        }
    );
    drop(registry);
    let (_, _, registry) = open_following(root.path(), runtime, b"three", configuration()).await;
    assert_eq!(
        running_generation(&registry.status().await.unwrap().plugins[0])
            .unwrap()
            .artifact,
        ContentHash::of(b"three")
    );
}

struct ConfiguredFactory;

#[async_trait]
impl Runtime for ConfiguredFactory {
    async fn load(&self, bytes: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError> {
        if bytes == COMPOSER_BYTES {
            return Ok(composer_loaded(Arc::new(TestComposer)));
        }
        if bytes == b"new"
            && settings
                .config
                .get("label")
                .is_some_and(|label| label != "new")
        {
            return Err(LoadError::Settings("new code requires label=new".into()));
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
    let old = running_generation(&first.status().await.unwrap().plugins[0])
        .unwrap()
        .id;
    promote(&first, old).await;
    drop(first);
    let (store, development, registry) =
        open_following(root.path(), runtime.clone(), b"new", document.clone()).await;
    let status = registry.status().await.unwrap();
    let plugin = &status.plugins[0];
    assert_eq!(registry.history(None, 2).await.unwrap().len(), 1);
    assert_eq!(running_generation(plugin).unwrap().id, old);
    let follow_rejection = development.status(&registry).await.unwrap().plugins["fixture"]
        .registered
        .as_ref()
        .unwrap()
        .follow_rejection
        .clone()
        .unwrap();
    assert!(
        matches!(follow_rejection.as_slice(), [cause @ Cause::Load { candidate: Proposed::Factory, .. }]
            if cause.part() == IntentPart::Settings { plugin: "fixture".parse().unwrap() })
    );
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry.clone(),
        None,
        Some(development.clone()),
    ))
    .await
    .unwrap();
    let complete = current_status(root.path(), &development, &kernel).await;
    let json = serde_json::to_value(&complete).unwrap();
    assert!(
        json["needs_attention"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "follow_rejected")
    );
    kernel.shutdown().await.unwrap();
    let factory = status.registered.plugins["fixture"]
        .factory
        .as_ref()
        .unwrap();
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
        .apply(
            &registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(applied.registered.unwrap().configuration.is_some());
    let plugin = registry.status().await.unwrap().plugins.remove(0);
    let generation = running_generation(&plugin).unwrap();
    assert_eq!(generation.artifact, ContentHash::of(b"new"));
    assert_eq!(generation.settings.config["label"], "new");
    assert!(
        development.status(&registry).await.unwrap().plugins["fixture"]
            .registered
            .as_ref()
            .unwrap()
            .follow_rejection
            .is_none()
    );
    assert_eq!(
        plugin.standing,
        Standing::Registered {
            differences: vec![]
        }
    );
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
        .open(registry_deps(store.clone(), runtime.clone(), b"two"))
        .await
        .unwrap();
    assert_eq!(
        running_generation(&registry.status().await.unwrap().plugins[0])
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
        registry.registered_intent(None).await.unwrap().plugins["fixture"]
            .code
            .as_ref()
            .map(|code| code.intent),
        Some(CodeIntent::Directory {
            source: enco_host::development::tree::FileTree::read(
                &root.path().join("plugins/fixture")
            )
            .await
            .unwrap()
            .hash()
        })
    );
    drop(registry);
    std::fs::remove_dir_all(root.path().join("plugins/fixture")).unwrap();
    let registry = development
        .open(registry_deps(store.clone(), runtime, b"three"))
        .await
        .unwrap();
    assert_eq!(
        running_generation(&registry.status().await.unwrap().plugins[0])
            .unwrap()
            .id,
        custom.id
    );
    development
        .apply(
            &registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        running_generation(&registry.status().await.unwrap().plugins[0])
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
        .apply(
            &registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        registry.registered_intent(None).await.unwrap().plugins["fixture"]
            .code
            .as_ref()
            .map(|code| code.intent),
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
        if bytes == COMPOSER_BYTES {
            return Ok(composer_loaded(Arc::new(TestComposer)));
        }
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
    let (store, development, registry) =
        open_following(root.path(), runtime.clone(), FACTORY_BYTES, configuration()).await;
    let baseline = registry
        .registered_intent(None)
        .await
        .unwrap()
        .registration()
        .unwrap();
    let cancel = CancellationToken::new();
    let directory = root.path().join("plugins/fixture");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("source.txt"), "paused").unwrap();
    retain_build(root.path(), &store, &directory, b"paused").await;
    let applying = development.apply(&registry, Initiator::Owner { reason: None }, &cancel);
    let inspecting = async {
        runtime.entered.notified().await;
        let status = development.status(&registry).await.unwrap();
        assert_eq!(status.recent_registrations[0].id, baseline);
        assert!(status.plugins_unreadable.is_none());
        runtime.resume.notify_one();
    };
    // Status must finish before loading is released; the deadline only bounds a deadlock.
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        tokio::join!(applying, inspecting)
    })
    .await
    .unwrap();
    assert!(result.unwrap().registered.unwrap().warnings.is_empty());
}

async fn apply_configuration(
    root: &Path,
    development: &Development,
    registry: &Registry,
    document: &serde_json::Value,
) -> enco_host::development::Applied {
    std::fs::write(root.join("config.toml"), toml::to_string(document).unwrap()).unwrap();
    development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
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
    assert_eq!(running_generation(plugin).unwrap().id, old.id);
    assert_eq!(
        plugin.standing,
        Standing::Registered {
            differences: vec![Difference::Code]
        }
    );
    let intended = &status.registered.plugins["fixture"].code.as_ref().unwrap();
    assert_eq!(intended.generation, broken.id);
    let rejected = &status.registered.plugins["fixture"].activation.generation;
    assert_eq!(rejected.id, intended.generation);
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
    let activation = plugin_status(&registry, old.plugin)
        .await
        .running
        .activation()
        .unwrap()
        .clone();
    assert_eq!(activation.generation.id, old.id);
    assert_eq!(activation.code.generation, old.id);

    document["run"] = serde_json::json!({"max_rounds": 31});
    let unrelated = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert!(unrelated.generations.is_empty() && unrelated.registration.names.is_empty());
    check_running_source_and_repair(root.path(), &store, &development, &registry, &running).await;
}

/// Status leads from the rolled-back code to the registration that holds its source.
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
    let activation = plugin.running.activation().unwrap();
    assert_eq!(activation.generation.id, old.id);
    assert_eq!(
        plugin.standing,
        Standing::Registered {
            differences: vec![Difference::Settings]
        }
    );
    assert_eq!(activation.code.generation, old.id);
    let supplier = running.registration.id;
    assert!(matches!(
        activation.code.source,
        CodeSource::Directory { registration, .. } if registration == supplier
    ));
    development
        .rewind(
            registry,
            enco_host::development::RewindArguments {
                registration: supplier,
                paths: Some(vec!["plugins/fixture".parse().unwrap()]),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(root.join("plugins/fixture/source.txt")).unwrap(),
        b"one"
    );
    let fixed = source_fixture(root, store, development, registry, "fixture", b"fixed")
        .await
        .generations
        .remove(0);
    assert_eq!(
        fixed.settings.config,
        serde_json::json!({"label": "settings"})
    );
    assert_eq!(
        plugin_status(registry, old.plugin).await.standing,
        Standing::Registered {
            differences: vec![]
        }
    );
}

#[tokio::test]
async fn settings_use_registered_names_while_running_names_remain_reserved() {
    let root = tempfile::tempdir().unwrap();
    let mut document = configuration();
    let (store, development, registry) = open_following(
        root.path(),
        runtime(&[FACTORY_BYTES, b"a", b"c"]),
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
        .rename(
            &registry,
            &"a".parse().unwrap(),
            &"b".parse().unwrap(),
            Initiator::Owner { reason: None },
            &cancel,
        )
        .await
        .unwrap();
    registry.rollback().await.unwrap();
    let grouped = development.status(&registry).await.unwrap();
    let running = grouped.plugins["b"].running.as_ref().unwrap();
    assert_eq!(
        running.standing,
        Standing::Registered {
            differences: vec![Difference::Name {
                running: "a".parse().unwrap()
            }]
        }
    );
    assert_eq!(
        grouped.plugins["b"].registered.as_ref().unwrap().plugin.id,
        old.plugin
    );
    assert!(!grouped.plugins.contains_key("a"));

    reject_settings_for_running_name(root.path(), &development, &registry, &mut document).await;

    document["plugin"] = serde_json::json!({"b": {"config": {"label": "registered name"}}});
    let settings = apply_configuration(root.path(), &development, &registry, &document)
        .await
        .registered
        .unwrap();
    assert_eq!(settings.generations[0].plugin, old.plugin);
    let plugin = plugin_status(&registry, old.plugin).await;
    assert_eq!(plugin.running.name().map(PluginName::as_str), Some("a"));
    let activation = plugin.running.activation().unwrap();
    assert_eq!(
        activation.generation.settings.config["label"],
        "registered name"
    );
    assert_eq!(activation.code.generation, old.id);
    source_fixture(root.path(), &store, &development, &registry, "c", b"c").await;
    let taken: PluginName = "a".parse().unwrap();
    let collision = development
        .rename(
            &registry,
            &"c".parse().unwrap(),
            &taken,
            Initiator::Owner { reason: None },
            &cancel,
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(&*collision.error,
        DevelopmentError::Registry(RegistryError::Rejected(causes))
        if matches!(causes.as_slice(), [Cause::NameTaken { name }] if *name == taken)));
    assert_eq!(collision.code(), code::PLUGIN_REJECTED);
    assert_eq!(
        collision.next().first,
        [Step::Fix(FixTarget::Name { name: taken })]
    );
    assert!(root.path().join("plugins/c").is_dir());
}

async fn reject_settings_for_running_name(
    root: &std::path::Path,
    development: &Development,
    registry: &Registry,
    document: &mut serde_json::Value,
) {
    let baseline = registry
        .registered_intent(None)
        .await
        .unwrap()
        .registration()
        .unwrap();
    document["plugin"] = serde_json::json!({"a": {"config": {"label": "wrong name"}}});
    std::fs::write(
        root.join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let failed = development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(&*failed.error,
        DevelopmentError::Registry(RegistryError::Rejected(causes))
        if matches!(causes.as_slice(), [Cause::SettingsWithoutPlugin { plugin }] if plugin.as_str() == "a")));
    assert_eq!(
        registry
            .registered_intent(None)
            .await
            .unwrap()
            .registration()
            .unwrap(),
        baseline
    );
    assert!(matches!(
        registry
            .check_rename(&"a".parse().unwrap(), &"unused".parse().unwrap())
            .await,
        Err(RegistryError::UnknownPlugin(_))
    ));
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
        .apply(
            &registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
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
    assert_eq!(running_generation(&restored).unwrap().id, x.id);
    assert_eq!(
        restored.running.name().map(PluginName::as_str),
        Some("custom")
    );
    let removed = plugin_status(&registry, y.plugin).await;
    assert!(matches!(removed.running, Running::Stopped));
}
