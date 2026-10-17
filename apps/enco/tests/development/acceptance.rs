use super::*;

#[tokio::test]
async fn first_start_requires_git_and_rejects_a_failed_source_build() {
    for missing_git in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path();
        let server = MockServer::start().await;
        std::fs::write(
            home.join("config.toml"),
            Daemon::config(&server, "openai-compatible"),
        )
        .unwrap();
        support::initialize(home).await;
        support::build_target(home, "bootstrap-failure");
        support::copy_directory(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/test-fixture"),
            &home.join("plugins/fixture"),
        );
        std::fs::write(
            home.join("plugins/fixture/src/lib.rs"),
            "fn invalid() { ??? }",
        )
        .unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"));
        command
            .arg("serve")
            .env("ENCO_HOME", home)
            .env("CARGO_NET_OFFLINE", "true")
            .kill_on_drop(true);
        if missing_git {
            command.env("PATH", home.join("absent-bin"));
        }
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if missing_git {
                "git rev-parse --show-toplevel"
            } else {
                "cargo clippy failed"
            }),
            "{error}"
        );
        let store = SqliteStore::open(StorePaths {
            db: home.join(".data/enco.db"),
            blobs: home.join(".data/blobs"),
            artifacts: home.join(".data/artifacts"),
        })
        .await
        .unwrap();
        let state = store.registry().await.unwrap();
        assert!(state.registrations.is_empty() && state.active_config.is_none());
    }
}

pub(super) async fn apply_preserving_owner_history(daemon: &Daemon) -> Value {
    let home = daemon.root.path();
    std::fs::write(home.join("owner-notes"), "Owner version").unwrap();
    git(home, &["add", "owner-notes"]).await;
    git(
        home,
        &[
            "-c",
            "user.name=Owner",
            "-c",
            "user.email=owner@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-m",
            "Owner history",
        ],
    )
    .await;
    std::fs::write(home.join("owner-notes"), "Staged owner edit").unwrap();
    git(home, &["add", "owner-notes", "config.toml"]).await;
    std::fs::write(home.join("owner-notes"), "Unstaged owner edit").unwrap();
    let index = std::fs::read(home.join(".git/index")).unwrap();
    let head = std::fs::read(home.join(".git/HEAD")).unwrap();
    let branches = git(home, &["for-each-ref", "refs/heads/"]).await;
    let applied = tool_results(daemon, REGISTER)
        .await
        .into_iter()
        .find(|value| value.get("builds").is_some())
        .unwrap();
    assert_eq!(std::fs::read(home.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(home.join(".git/HEAD")).unwrap(), head);
    assert_eq!(git(home, &["for-each-ref", "refs/heads/"]).await, branches);
    assert_eq!(
        std::fs::read_to_string(home.join("owner-notes")).unwrap(),
        "Unstaged owner edit"
    );
    let revision = applied["registered"]["registration"]["revision"]
        .as_str()
        .unwrap();
    assert_eq!(
        git(
            home,
            &["show", "-s", "--format=%an <%ae>%n%cn <%ce>", revision]
        )
        .await,
        "Enco <enco@localhost>\nEnco <enco@localhost>"
    );
    let tree = git(home, &["rev-parse", &format!("{revision}:plugins/alpha")]).await;
    let record: (String, String, String) = rusqlite::Connection::open(home.join(".data/enco.db"))
        .unwrap()
        .query_row(
            "SELECT sdk,artifact,toolchain FROM builds WHERE source=?",
            [&tree],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(record.0, super::sdk::SDK.hash().to_string());
    assert_eq!(
        record.1,
        applied["registered"]["generations"][0]["artifact"]
            .as_str()
            .unwrap()
    );
    assert!(record.2.contains("rustc ") && record.2.contains("host:"));
    applied
}

#[tokio::test]
async fn restoring_older_source_rebuilds_pruned_bytes_and_deletion_survives_restart() {
    let service = echo_service().await;
    let mut daemon = Daemon::start(&service, "openai-compatible").await;
    let home = daemon.root.path().to_path_buf();
    support::build_target(&home, "old-source");
    daemon
        .connect()
        .await
        .request(Command::PluginScaffold {
            name: "alpha".into(),
        })
        .await
        .unwrap();
    let source = home.join("plugins/alpha/src/lib.rs");
    let template = std::fs::read_to_string(&source).unwrap();
    let mut versions = Vec::new();
    for label in ["one", "two", "three"] {
        std::fs::write(&source, template.replace("Echo an input object", label)).unwrap();
        let applied = apply(&daemon).await;
        let status = use_source(&daemon).await;
        versions.push((
            status["registered"]["base"]["revision"]
                .as_str()
                .unwrap()
                .to_owned(),
            applied["registered"]["generations"][0].clone(),
        ));
    }
    let original = &versions[0].1;
    let hash = original["artifact"].as_str().unwrap();
    assert!(!home.join(format!(".data/artifacts/{hash}.wasm")).exists());
    git(
        &home,
        &[
            "restore",
            "--source",
            &versions[0].0,
            "--",
            "plugins/alpha",
            "config.toml",
        ],
    )
    .await;
    let restored = apply(&daemon).await;
    assert_eq!(restored["builds"][0]["reused"], false);
    assert_eq!(
        restored["registered"]["generations"][0]["artifact"],
        original["artifact"]
    );
    use_source(&daemon).await;
    delete_and_recreate(&mut daemon, &service, original, &versions[0].0).await;
    daemon.stop().await;
}

async fn delete_and_recreate(
    daemon: &mut Daemon,
    service: &MockServer,
    original: &Value,
    revision: &str,
) {
    let home = daemon.root.path().to_path_buf();
    let before = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    let requests_before = service.received_requests().await.unwrap().len();
    std::fs::remove_dir_all(home.join("plugins/alpha")).unwrap();
    let deleted = apply(daemon).await;
    assert!(deleted["registered"]["registration"].is_object());
    let tool_absent = tool_results(daemon, "exercise the plugin and inspect the node").await;
    let status = tool_absent
        .iter()
        .find(|value| value.get("host").is_some())
        .unwrap();
    let plugin = status["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == original["plugin"])
        .unwrap();
    assert!(plugin["active"].is_null() && plugin["name"].is_null());
    let requests = service.received_requests().await.unwrap();
    let request = requests
        .iter()
        .skip(requests_before)
        .find(|request| request.url.path() == "/chat/completions")
        .unwrap();
    let request: Value = serde_json::from_slice(&request.body).unwrap();
    assert!(
        request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["function"]["name"] != "alpha_echo")
    );
    daemon.stop().await;
    daemon.restart().await;
    let status = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    let plugin = status["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == original["plugin"])
        .unwrap();
    assert!(plugin["active"].is_null());
    let old = before["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == original["plugin"])
        .unwrap();
    assert_eq!(plugin["generations"], old["generations"]);
    // Recreating the accepted source still acquires a new identity, even with identical bytes.
    git(
        &home,
        &["restore", "--source", revision, "--", "plugins/alpha"],
    )
    .await;
    let added = apply(daemon).await;
    assert_eq!(added["builds"][0]["reused"], true);
    assert_ne!(
        added["registered"]["generations"][0]["plugin"],
        original["plugin"]
    );
}

const PROBE_ACCEPTED: &str = "        Ok(())";
const PROBE_REJECTED: &str = concat!(
    "        Err(types::Failure { code: \"plugin.contract\".into(), ",
    "message: \"fixture probe rejected\".into(), retryable: false })"
);

#[tokio::test]
async fn a_failed_probe_is_rejected_by_both_source_and_cli_artifact_registration() {
    let server = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let home = daemon.root.path().to_path_buf();
    support::build_target(&home, "probe-rejection");
    support::copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/test-fixture"),
        &home.join("plugins/fixture"),
    );
    let source = home.join("plugins/fixture/src/lib.rs");
    let text = std::fs::read_to_string(&source)
        .unwrap()
        .replace(PROBE_ACCEPTED, PROBE_REJECTED);
    std::fs::write(source, text).unwrap();
    let before = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    let rejected = daemon
        .connect()
        .await
        .request(Command::Apply {})
        .await
        .unwrap_err()
        .to_string();
    assert!(rejected.contains("fixture probe rejected"), "{rejected}");
    std::fs::copy(
        home.join(".data/target/wasm32-wasip2/release/test_fixture.wasm"),
        home.join("rejected.wasm"),
    )
    .unwrap();
    std::fs::remove_dir_all(home.join("plugins/fixture")).unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["plugin", "deploy", "fixture", "rejected.wasm"])
        .current_dir(&home)
        .env("ENCO_HOME", &home)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fixture probe rejected"));
    let after = daemon
        .connect()
        .await
        .request(Command::Status {})
        .await
        .unwrap();
    assert_eq!(after["registered"], before["registered"]);
    assert_eq!(after["plugins"], before["plugins"]);
    assert_eq!(
        git(&home, &["rev-parse", "enco/registered"]).await,
        before["registered"]["base"]["revision"].as_str().unwrap()
    );
    daemon.stop().await;
}
