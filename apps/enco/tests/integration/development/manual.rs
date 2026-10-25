use super::*;
use enco_host::development::tree::FileTree;

#[tokio::test]
async fn the_manual_follows_what_runs_and_returns_without_cargo() {
    let service = echo_service().await;
    let mut daemon = Daemon::start(&service, "openai-compatible").await;
    let home = daemon.root.path().to_path_buf();
    support::build_target(&home, "manual");
    let manual = home.join("manual");
    for path in [
        "README.md",
        "contract.md",
        "sdk/API.md",
        "host/enco-core/API.md",
        "plugins/openai-compatible/README.md",
    ] {
        assert!(manual.join(path).is_file(), "{path}");
    }
    assert!(!manual.join("plugins/openai-compatible/code").exists());

    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "alpha".parse().unwrap(),
        })
        .await
        .unwrap();
    apply(&daemon).await;
    let read = |path: &str| std::fs::read_to_string(manual.join(path)).unwrap();
    assert!(read("README.md").contains("[alpha](plugins/alpha/README.md)"));
    assert!(read("plugins/alpha/README.md").contains("### `alpha_echo`"));
    assert!(read("plugins/alpha/code/README.md").starts_with("# crate `plugin_alpha`"));
    // The code a plugin author writes lives mostly in the trait implementations.
    assert!(read("plugins/alpha/code/crate.md").contains("### `<Plugin as Guest>::call`"));

    // The manual is derived state: without it, and without Cargo, a restart writes it again.
    let written = FileTree::read(&manual).await.unwrap().hash();
    std::fs::remove_dir_all(&manual).unwrap();
    daemon.stop().await;
    daemon.restart_without(&["PATH"]).await;
    assert_eq!(FileTree::read(&manual).await.unwrap().hash(), written);
    daemon.stop().await;
}
