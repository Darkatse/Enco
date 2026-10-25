use super::*;

async fn cli(daemon: &Daemon, arguments: &[&str]) -> String {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(arguments)
        .env("ENCO_HOME", daemon.root.path())
        .env("PATH", support::without_git(daemon.root.path()))
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test]
async fn cli_requests_documents_and_json_from_the_daemon_without_git() {
    let service = echo_service().await;
    let mut daemon = Daemon::start(&service, "openai-compatible").await;
    let initial: Value = serde_json::from_str(&cli(&daemon, &["status", "--json"]).await).unwrap();
    let registration = initial["registration"].to_string();
    let path = daemon.root.path().join("config.toml");
    let original = std::fs::read(&path).unwrap();
    std::fs::write(
        &path,
        format!(
            "{}\n[run]\nmax_rounds = 23\n",
            String::from_utf8_lossy(&original)
        ),
    )
    .unwrap();
    for arguments in [
        vec!["status"],
        vec!["history", "--to", "home"],
        vec!["apply", "--reason", "Allow a longer run"],
        vec!["history"],
        vec!["rewind", &registration],
    ] {
        let document = cli(&daemon, &arguments).await;
        assert!(
            serde_json::from_str::<Value>(&document).is_err(),
            "{document}"
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let history: Value = serde_json::from_str(&cli(&daemon, &["history", "--json"]).await).unwrap();
    assert_eq!(
        history["registrations"][0]["initiator"]["reason"],
        "Allow a longer run"
    );
    daemon.stop().await;
}
