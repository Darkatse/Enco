use super::*;

#[tokio::test]
async fn first_start_rejects_a_failed_source_build() {
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
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("cargo clippy"), "{error}");
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

#[tokio::test]
async fn first_start_checks_the_pinned_toolchain_before_cargo() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let server = MockServer::start().await;
    std::fs::write(
        home.join("config.toml"),
        Daemon::config(&server, "openai-compatible"),
    )
    .unwrap();
    support::initialize(home).await;
    support::copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/test-fixture"),
        &home.join("plugins/fixture"),
    );
    let toolchain: toml::Table = toml::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-toolchain.toml"),
        )
        .unwrap(),
    )
    .unwrap();
    let channel = toolchain["toolchain"]["channel"].as_str().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let cargo_ran = tools.path().join("cargo-ran");
    script(
        &tools.path().join("cargo"),
        &format!("touch '{}'\nexit 1", cargo_ran.display()),
    );
    // A sysroot without the plugin target.
    let sysroot = tempfile::tempdir().unwrap();
    fake_rustc(tools.path(), "1.0.0", sysroot.path());
    let error = serve_with_tools(home, tools.path()).await;
    assert!(
        error.contains(&format!(
            "builds with Rust {channel}, but `rustc` is release 1.0.0"
        )),
        "{error}"
    );
    fake_rustc(tools.path(), channel, sysroot.path());
    let error = serve_with_tools(home, tools.path()).await;
    assert!(
        error.contains("lacks the `wasm32-wasip2` target")
            && error.contains(&format!(
                "`rustup toolchain install {channel} --profile minimal --component clippy --target wasm32-wasip2`"
            )),
        "{error}"
    );
    assert!(!cargo_ran.exists());
}

/// Start the daemon with `tools` first on `PATH` and its `rustc` as `RUSTC`, expecting it to fail.
async fn serve_with_tools(home: &Path, tools: &Path) -> String {
    let path = std::env::join_paths(
        std::iter::once(tools.to_owned())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("serve")
        .env("ENCO_HOME", home)
        .env("PATH", path)
        .env("RUSTC", tools.join("rustc"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(60), output)
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn fake_rustc(tools: &Path, release: &str, sysroot: &Path) {
    script(
        &tools.join("rustc"),
        &format!(
            "case \"$1\" in\n-vV) printf 'rustc {release}\\nrelease: {release}\\n' ;;\n--print) printf '%s\\n' '{}' ;;\nesac",
            sysroot.display()
        ),
    );
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub(super) async fn register_with_agent(daemon: &Daemon) -> Value {
    let entries = tool_results(daemon, REGISTER).await;
    let call = entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                call,
                outcome: Settlement::Ok,
                ..
            } => Some(*call),
            _ => None,
        })
        .unwrap();
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    let registered = status["recent_registrations"][0].clone();
    assert_eq!(registered["initiator"]["kind"], "agent");
    assert_eq!(registered["initiator"]["call"], json!(call));
    let log = daemon
        .connect()
        .await
        .request(Command::Log {
            session: registered["initiator"]["session"]["id"]
                .as_str()
                .unwrap()
                .into(),
            after: None,
        })
        .await
        .unwrap();
    let log: Vec<Entry> = serde_json::from_value(log).unwrap();
    assert!(log.iter().any(
        |entry| matches!(entry.body, EntryBody::ToolCallStarted { call: id, .. } if id == call)
    ));
    assert_eq!(
        registered["initiator"]["reason"],
        "Register source and configuration"
    );
    let home = daemon.root.path();
    let source = enco_host::development::tree::FileTree::read(&home.join("plugins/alpha"))
        .await
        .unwrap()
        .hash();
    let record: (String, String, String) = rusqlite::Connection::open(home.join(".data/enco.db"))
        .unwrap()
        .query_row(
            "SELECT sdk,artifact,toolchain FROM builds WHERE source=?",
            [source.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(record.0, super::sdk::sdk().unwrap().hash().to_string());
    assert_eq!(
        record.1,
        registered["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["kind"] == "code" && change["name"] == "alpha")
            .unwrap()["generation"]["artifact"]
            .as_str()
            .unwrap()
    );
    assert!(record.2.contains("rustc ") && record.2.contains("host:"));
    registered
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
            name: "alpha".parse().unwrap(),
        })
        .await
        .unwrap();
    let source = home.join("plugins/alpha/src/lib.rs");
    let template = std::fs::read_to_string(&source).unwrap();
    let mut versions = Vec::new();
    for label in ["one", "two", "three"] {
        std::fs::write(&source, template.replace("Echo the given text", label)).unwrap();
        let applied = apply(&daemon).await;
        use_source(&daemon).await;
        versions.push((
            serde_json::from_value::<enco_core::RegistrationId>(
                applied["registered"]["registration"]["id"].clone(),
            )
            .unwrap(),
            applied["registered"]["generations"][0].clone(),
        ));
    }
    let original = &versions[0].1;
    let hash = original["artifact"].as_str().unwrap();
    assert!(!home.join(format!(".data/artifacts/{hash}.wasm")).exists());
    daemon
        .connect()
        .await
        .request(Command::Rewind {
            output: protocol::Output::Json,
            arguments: enco_host::development::RewindArguments {
                registration: versions[0].0,
                paths: None,
            },
        })
        .await
        .unwrap();
    let restored = apply(&daemon).await;
    assert_eq!(restored["builds"][0]["reused"], false);
    assert_eq!(
        restored["registered"]["generations"][0]["artifact"],
        original["artifact"]
    );
    use_source(&daemon).await;
    delete_and_recreate(&mut daemon, &service, original, versions[0].0).await;
    daemon.stop().await;
}

async fn delete_and_recreate(
    daemon: &mut Daemon,
    service: &MockServer,
    original: &Value,
    registration: enco_core::RegistrationId,
) {
    let home = daemon.root.path().to_path_buf();
    let requests_before = service.received_requests().await.unwrap().len();
    std::fs::remove_dir_all(home.join("plugins/alpha")).unwrap();
    let deleted = apply(daemon).await;
    assert!(deleted["registered"]["registration"].is_object());
    tool_results(daemon, "exercise the plugin and inspect the node").await;
    let status = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert!(status["plugins"].get("alpha").is_none());
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
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert!(status["plugins"].get("alpha").is_none());
    // Recreating the accepted source still acquires a new identity, even with identical bytes.
    daemon
        .connect()
        .await
        .request(Command::Rewind {
            output: protocol::Output::Json,
            arguments: enco_host::development::RewindArguments {
                registration,
                paths: Some(vec!["plugins/alpha".parse().unwrap()]),
            },
        })
        .await
        .unwrap();
    let added = apply(daemon).await;
    assert_eq!(added["builds"][0]["reused"], true);
    assert_ne!(
        added["registered"]["generations"][0]["plugin"],
        original["plugin"]
    );
}

#[tokio::test]
async fn a_tool_schema_outside_the_subset_is_rejected_with_its_location() {
    let server = MockServer::start().await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
    let home = daemon.root.path().to_path_buf();
    support::build_target(&home, "schema-rejection");
    support::copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/test-fixture"),
        &home.join("plugins/fixture"),
    );
    let source = home.join("plugins/fixture/src/lib.rs");
    // Forgetting `deny_unknown_fields` leaves the object open, which the subset rejects.
    let text = std::fs::read_to_string(&source).unwrap().replace(
        "#[serde(deny_unknown_fields)]\nstruct Spin {}",
        "struct Spin {}",
    );
    std::fs::write(source, text).unwrap();
    let rejected = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(
        rejected.contains(
            "tool fixture_spin schema: schema root: an object must set `additionalProperties` to false"
        ),
        "{rejected}"
    );
    daemon.stop().await;
}

const PROBE_ACCEPTED: &str = "        Ok(())";
const PROBE_REJECTED: &str = concat!(
    "        Err(types::Failure { code: \"plugin.contract\".into(), ",
    "message: \"fixture probe rejected\".into(), retryable: false })"
);

#[tokio::test]
async fn a_failed_probe_is_rejected_before_source_registration() {
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
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    let rejected = daemon
        .connect()
        .await
        .request(Command::Apply {
            output: protocol::Output::Json,
            reason: None,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(rejected.contains("fixture probe rejected"), "{rejected}");
    let after = daemon
        .connect()
        .await
        .request(Command::Status {
            output: protocol::Output::Json,
        })
        .await
        .unwrap();
    assert_eq!(
        after["recent_registrations"],
        before["recent_registrations"]
    );
    for (name, unit) in before["plugins"].as_object().unwrap() {
        assert_eq!(after["plugins"][name]["running"], unit["running"]);
        assert_eq!(after["plugins"][name]["registered"], unit["registered"]);
    }
    daemon.stop().await;
}
