use super::*;
use enco_host::development;

/// Host-facing tests use the real intent chain while replacing only the plugin runtime.
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
    let development = development::Development::new(
        root.into(),
        development::Sdk { files: &[] },
        store,
        deps.runtime.clone(),
    );
    deps.host_tool_names
        .extend(development_tool_specs().into_iter().map(|spec| spec.name));
    let registry = development.open(deps, "fixture").await.unwrap();
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
    let scratch = tempfile::tempdir().unwrap();
    let index = scratch.path().join("index");
    for args in [vec!["read-tree", "--empty"], vec!["add", "--", &relative]] {
        let status = tokio::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .env("GIT_INDEX_FILE", &index)
            .args(args)
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .env("GIT_INDEX_FILE", &index)
        .arg("write-tree")
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let tree = String::from_utf8(output.stdout).unwrap();
    let source = git(root, &["rev-parse", &format!("{}:{relative}", tree.trim())]).await;
    let artifact = store.put_artifact(bytes).await.unwrap();
    rusqlite::Connection::open(root.join("enco.db"))
        .unwrap()
        .execute(
            "INSERT INTO builds(source,sdk,artifact,toolchain,created_at) VALUES (?,?,?,?,?)",
            rusqlite::params![
                source,
                development::Sdk { files: &[] }.hash().to_string(),
                artifact.to_string(),
                "scripted compiler",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
    let applied = development
        .apply(registry, &tokio_util::sync::CancellationToken::new())
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
    // These fixtures keep their store directly under the test root.
    std::fs::write(
        root.join(".gitignore"),
        "enco.db*\n/blobs/\n/artifacts/\nmemory*\n",
    )
    .unwrap();
    development::Development::initialize(root).await.unwrap();
}

pub async fn git(root: &Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
