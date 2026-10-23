use super::*;
use enco_core::Initiator;
use enco_host::{
    SystemClock,
    development::{Development, Sdk},
};
use enco_kernel::{FactoryPlugin, RegistryDeps};
use std::collections::BTreeSet;

include!(concat!(env!("OUT_DIR"), "/sdk.rs"));
include!(concat!(env!("OUT_DIR"), "/embedded_factory.rs"));

/// Each test host's SDK builds observably different plugins.
const LABEL_FUNCTION: &str =
    "/// Label returned by the test host's SDK.\npub fn test_revision() -> &'static str";
const ECHO: &str = "types::Outcome::Ok(input)";
const LABELLED_ECHO: &str = concat!(
    r##"types::Outcome::Ok(format!(r#"{{"label":"{}","input":{input}}}"#, "##,
    "enco_sdk::test_revision()))"
);
const DIRECT_ARTIFACT: &str = "plugins/sdk-test/target/wasm32-wasip2/release/plugin_sdk_test.wasm";

fn with_label(label: &str) -> Sdk {
    let files = SDK_FILES
        .iter()
        .map(|(path, bytes)| {
            if *path != "src/lib.rs" {
                return (*path, bytes.to_vec());
            }
            let library = std::str::from_utf8(bytes).unwrap();
            let source = format!("{library}\n{LABEL_FUNCTION} {{ {label:?} }}\n");
            (*path, source.into_bytes())
        })
        .collect::<Vec<_>>();
    Sdk::new(
        &files
            .iter()
            .map(|(path, bytes)| (*path, bytes.as_slice()))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[tokio::test]
async fn sdk_upgrades_and_downgrades_compare_built_artifacts_and_unchanged_sdk_skips_cargo() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let server = MockServer::start().await;
    std::fs::write(
        home.join("config.toml"),
        Daemon::config(&server, "openai-compatible"),
    )
    .unwrap();
    support::initialize(home).await;
    support::build_target(home, "sdk");
    let runtime = Arc::new(WasmRuntime::new().unwrap());
    let store = Arc::new(
        SqliteStore::open(StorePaths {
            db: home.join(".data/enco.db"),
            blobs: home.join(".data/blobs"),
            artifacts: home.join(".data/artifacts"),
        })
        .await
        .unwrap(),
    );
    let (old, registry) = open(home, with_label("one"), store.clone(), runtime.clone()).await;
    old.scaffold(&registry, &"sdk-test".parse().unwrap())
        .await
        .unwrap();
    let source = home.join("plugins/sdk-test/src/lib.rs");
    let text = std::fs::read_to_string(&source)
        .unwrap()
        .replace(ECHO, LABELLED_ECHO);
    std::fs::write(&source, text).unwrap();
    let cancel = CancellationToken::new();
    let first = old
        .apply(&registry, Initiator::Owner { reason: None }, &cancel)
        .await
        .unwrap();
    assert!(!first.builds[0].reused);
    assert_lock_keeps_sdk_versions(home);
    let artifact = first.registered.as_ref().unwrap().generations[0].artifact;
    let before = old.status(&registry).await.unwrap();
    build_from_directory(home).await;
    let after = old.status(&registry).await.unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    for _ in 0..enco_kernel::TRIAL_CALLS {
        registry
            .report(
                first.registered.as_ref().unwrap().generations[0].id,
                enco_kernel::Verdict::Ok,
                None,
            )
            .await
            .unwrap();
    }
    assert!(!earlier_sdk(&old, &registry).await);
    // Empty build evidence proves the unchanged-source path never invokes Cargo.
    let unchanged = old
        .apply(&registry, Initiator::Owner { reason: None }, &cancel)
        .await
        .unwrap();
    assert!(unchanged.builds.is_empty() && unchanged.registered.is_none());
    // Two owners model switching hosts back and forth while retaining the same registry.
    let newer = Development::new(
        home.into(),
        with_label("two"),
        store.clone(),
        runtime.clone(),
    );
    assert!(earlier_sdk(&newer, &registry).await);
    let upgraded = newer
        .apply(&registry, Initiator::Owner { reason: None }, &cancel)
        .await
        .unwrap();
    assert!(!upgraded.builds[0].reused);
    assert_ne!(
        upgraded.registered.unwrap().generations[0].artifact,
        artifact
    );
    assert!(!earlier_sdk(&newer, &registry).await);
    assert!(earlier_sdk(&old, &registry).await);
    let downgraded = old
        .apply(&registry, Initiator::Owner { reason: None }, &cancel)
        .await
        .unwrap();
    assert!(downgraded.builds[0].reused);
    assert_eq!(
        downgraded.registered.unwrap().generations[0].artifact,
        artifact
    );
    assert!(!earlier_sdk(&old, &registry).await);
    assert!(
        old.apply(&registry, Initiator::Owner { reason: None }, &cancel)
            .await
            .unwrap()
            .builds
            .is_empty()
    );
}

async fn earlier_sdk(development: &Development, registry: &enco_kernel::Registry) -> bool {
    development.status(registry).await.unwrap().plugins["sdk-test"]
        .registered
        .as_ref()
        .unwrap()
        .earlier_sdk
}

async fn open(
    home: &Path,
    sdk: Sdk,
    store: Arc<SqliteStore>,
    runtime: Arc<WasmRuntime>,
) -> (Arc<Development>, Arc<enco_kernel::Registry>) {
    let old = Development::new(home.into(), sdk, store.clone(), runtime.clone());
    let registry = old
        .open(RegistryDeps {
            store: store.clone(),
            runtime: runtime.clone(),
            host_tool_names: Default::default(),
            factory: FACTORY
                .iter()
                .map(|(name, id, bytes)| FactoryPlugin {
                    name: (*name).parse().unwrap(),
                    id: id.parse().unwrap(),
                    artifact: bytes.to_vec(),
                })
                .collect(),
            owners: enco_host::TABLE_OWNERS.into(),
            clock: Arc::new(SystemClock),
        })
        .await
        .unwrap();
    (old, registry)
}

/// An owner's direct `cargo build` in the plugin directory, without Enco's target override.
async fn build_from_directory(home: &Path) {
    // Seed the plugin's own target directory from this scenario's cache, so Cargo only relinks.
    support::copy_directory(
        &home.join(".data/target"),
        &home.join("plugins/sdk-test/target"),
    );
    let output = tokio::process::Command::new("cargo")
        .current_dir(home.join("plugins/sdk-test"))
        .args(["build", "--release", "--message-format=json"])
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let messages: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // The seeded cache already holds the file, so only Cargo's report shows where it built.
    assert!(
        messages
            .iter()
            .filter_map(|message| message["filenames"].as_array())
            .flatten()
            .filter_map(Value::as_str)
            .any(|path| Path::new(path).ends_with(DIRECT_ARTIFACT)),
        "Cargo did not report {DIRECT_ARTIFACT}"
    );
    let directory = home.join("plugins/sdk-test");
    let before = enco_host::development::tree::FileTree::read(&directory)
        .await
        .unwrap()
        .hash();
    std::fs::write(home.join(DIRECT_ARTIFACT), b"ignored build output").unwrap();
    assert_eq!(
        enco_host::development::tree::FileTree::read(&directory)
            .await
            .unwrap()
            .hash(),
        before
    );
}

fn assert_lock_keeps_sdk_versions(home: &Path) {
    let versions = |lock: &str| -> BTreeSet<(String, String)> {
        let lock: toml::Table = toml::from_str(lock).unwrap();
        lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .map(|package| {
                let field = |name: &str| package[name].as_str().unwrap().to_owned();
                (field("name"), field("version"))
            })
            .collect()
    };
    let sdk = versions(std::str::from_utf8(sdk().unwrap().lock().unwrap()).unwrap());
    let plugin =
        versions(&std::fs::read_to_string(home.join("plugins/sdk-test/Cargo.lock")).unwrap());
    let missing: Vec<_> = sdk.difference(&plugin).collect();
    assert!(missing.is_empty(), "first build changed {missing:?}");
}
