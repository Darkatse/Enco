//! Profiles bind options to the composer generation actually selected for each composition.
use super::{fault, loaded, promote};
use crate::support::*;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use serde_json::json;
use std::sync::Arc;

fn composer_schema(field: &str, kind: &str) -> Loaded {
    let mut loaded = composer_loaded(Arc::new(TestComposer));
    loaded.composer.as_mut().unwrap().schema = InputSchema::parse(&json!({
        "type": "object", "additionalProperties": false,
        "properties": {field: {"type": kind}}, "required": []
    }))
    .unwrap();
    loaded
}

fn rejects_profile_options(result: Result<Registered, RegistryError>, field: &str) {
    assert!(matches!(result, Err(RegistryError::Rejected(causes))
        if matches!(causes.as_slice(), [Cause::Use {
            user, target: UseRef::Composer { profile }, failure,
        }] if user == "profile default.composer" && profile == "default"
            && failure.message.contains("options") && failure.message.contains(field))));
}

#[tokio::test]
async fn registrations_check_profile_options_against_candidate_composer_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    runtime.insert(COMPOSER_BYTES, composer_schema("limit", "integer"));
    runtime.insert(b"without-limit", composer_loaded(Arc::new(TestComposer)));
    let mut document = configuration();
    document["profile"]["default"]["composer"] = json!({
        "plugin": "composer", "options": {"limit": 5.0}
    });
    let registry = open_registry(
        registry_deps(store.clone(), runtime, FACTORY_BYTES),
        configuration_request(document.clone()),
    )
    .await
    .unwrap();
    let view = registry.view();
    let settings = &view.config.kernel.profiles["default"].composer;
    assert_eq!(
        serde_json::Value::Object(view.exports.composer(settings, false).unwrap().options),
        json!({"limit": 5})
    );
    let before = store.registry().await.unwrap();
    document["profile"]["default"]["composer"]["options"]["limit"] = "invalid".into();
    rejects_profile_options(
        submit(&registry, configuration_request(document)).await,
        "limit",
    );
    rejects_profile_options(
        submit(&registry, source_request("composer", b"without-limit")).await,
        "limit",
    );
    let after = store.registry().await.unwrap();
    assert_eq!(after.generations, before.generations);
    assert_eq!(after.active_config, before.active_config);
}

#[tokio::test]
async fn pinned_options_follow_current_code_and_unrepaired_mismatches_do_not_block_registration() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(store_paths(dir.path())).await.unwrap());
    let runtime = Arc::new(ScriptedRuntime::default());
    runtime.insert(FACTORY_BYTES, loaded(ScriptedProvider::new(vec![])));
    runtime.insert(b"style", composer_schema("style", "string"));
    runtime.insert(b"mood", composer_schema("mood", "string"));
    runtime.insert(b"tone", composer_schema("tone", "string"));
    let registry = super::open(&store, runtime).await;
    let mut document = configuration();
    document["profile"]["default"]["composer"] = json!({
        "plugin": "custom-composer", "options": {"style": "quiet"}
    });
    let mut request = configuration_request(document.clone());
    request.plugins = source_request("custom-composer", b"style").plugins;
    let old = submit(&registry, request)
        .await
        .unwrap()
        .generations
        .remove(0);
    promote(&registry, old.id).await;
    let pinned = registry.view().config.kernel.profiles["default"]
        .composer
        .clone();
    let factory = registry
        .view()
        .exports
        .composer(&pinned, true)
        .unwrap()
        .export
        .generation;
    document["profile"]["default"]["composer"]["options"] = json!({"mood": "bright"});
    let mut request = configuration_request(document);
    request.plugins = source_request("custom-composer", b"mood").plugins;
    let registered = submit(&registry, request).await.unwrap();
    assert!(registered.probes.is_empty());
    assert_eq!(registered.generations[0].status, GenerationStatus::Trial);
    let current = registry.view().config.kernel.profiles["default"]
        .composer
        .clone();
    let view = registry.view();
    assert!(
        matches!(view.exports.composer(&pinned, false), Err(failure) if failure.message.contains("style"))
    );
    assert_eq!(
        serde_json::Value::Object(view.exports.composer(&current, false).unwrap().options),
        json!({"mood": "bright"})
    );
    registry
        .report(
            registered.generations[0].id,
            Verdict::Failed(fault(code::PLUGIN_TRAP)),
            None,
        )
        .await
        .unwrap();
    let view = registry.view();
    assert!(
        matches!(view.exports.composer(&current, false), Err(failure) if failure.message.contains("mood"))
    );
    let safe = view.exports.composer(&current, true).unwrap();
    assert_eq!(safe.export.generation, factory);
    assert!(safe.options.is_empty());
    let unavailable = |status: RegistryStatus| {
        matches!(status.config.unavailable_uses.as_slice(), [usage]
            if usage.user == "profile default.composer"
                && usage.target == (UseRef::Composer { profile: "default".into() }))
    };
    assert!(unavailable(registry.status().await.unwrap()));
    // New code for the plugin may land while the mismatch it leaves unrepaired stays reported.
    submit(&registry, source_request("custom-composer", b"tone"))
        .await
        .unwrap();
    assert!(unavailable(registry.status().await.unwrap()));
}
