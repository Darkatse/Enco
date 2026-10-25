use super::*;
use enco_host::development;

/// An SDK with no sources; builds here are retained, never compiled.
fn sdk() -> development::Sdk {
    development::Sdk::new(&[(
        "rust-toolchain.toml",
        b"[toolchain]\nchannel = \"1.99.0\"\n",
    )])
    .unwrap()
}

/// Host-facing tests use the real content store while replacing only the plugin runtime.
pub async fn open_home(
    root: &Path,
    store: Arc<SqliteStore>,
    mut deps: RegistryDeps,
    document: serde_json::Value,
) -> (Arc<development::Development>, Arc<Registry>) {
    if !root.join("config.toml").exists() {
        prepare_home(root, &document).await;
    } else {
        development::Development::initialize(root).await.unwrap();
    }
    let development =
        development::Development::new(root.into(), sdk(), store, deps.runtime.clone());
    deps.host_tool_names.extend(
        development_tool_specs()
            .unwrap()
            .into_iter()
            .map(|spec| spec.name),
    );
    let registry = development.open(deps).await.unwrap();
    (development, registry)
}

/// Replace compilation with a retained build, then use the real registration path.
pub async fn source_fixture(
    root: &Path,
    store: &SqliteStore,
    development: &development::Development,
    registry: &Registry,
    name: &str,
    bytes: &[u8],
) -> Registered {
    let relative = format!("plugins/{name}");
    let directory = root.join(&relative);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("source.txt"), bytes).unwrap();
    retain_build(root, store, &directory, bytes).await;
    let applied = development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let registered = applied.registered.unwrap();
    assert!(registered.warnings.is_empty());
    registered
}

pub async fn scripted_home(
    root: &Path,
    runtime: Arc<dyn Runtime>,
    document: serde_json::Value,
    extra_tools: Vec<ToolSpec>,
) -> (
    Arc<development::Development>,
    Arc<Registry>,
    Arc<SqliteStore>,
) {
    let store = Arc::new(SqliteStore::open(store_paths(root)).await.unwrap());
    let mut deps = registry_deps(store.clone(), runtime, FACTORY_BYTES);
    deps.host_tool_names
        .extend(extra_tools.into_iter().map(|spec| spec.name));
    let (development, registry) = open_home(root, store.clone(), deps, document).await;
    (development, registry, store)
}

pub async fn prepare_home(root: &Path, document: &serde_json::Value) {
    std::fs::write(root.join("config.toml"), toml::to_string(document).unwrap()).unwrap();
    development::Development::initialize(root).await.unwrap();
}

pub async fn retain_build(root: &Path, store: &SqliteStore, directory: &Path, bytes: &[u8]) {
    let source = development::tree::FileTree::read(directory)
        .await
        .unwrap()
        .hash();
    let artifact = store.put_artifact(bytes).await.unwrap();
    rusqlite::Connection::open(root.join("enco.db"))
        .unwrap()
        .execute(
            "INSERT INTO builds(source,sdk,artifact,toolchain,created_at) VALUES (?,?,?,?,?) ON CONFLICT(source,sdk) DO UPDATE SET artifact=excluded.artifact,toolchain=excluded.toolchain,created_at=excluded.created_at",
            rusqlite::params![
                source.to_string(),
                sdk().hash().to_string(),
                artifact.to_string(),
                "scripted compiler",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
}

pub async fn current_status(
    root: &Path,
    development: &development::Development,
    kernel: &Kernel,
) -> enco_host::status::Status {
    enco_host::status::Status {
        development: development.status(kernel.registry()).await.unwrap(),
        node: enco_host::status::Node {
            version: "test".into(),
            revision: "fixture".into(),
            home: root.into(),
            execution: kernel.status().await.unwrap(),
            channels: vec![],
        },
    }
}
