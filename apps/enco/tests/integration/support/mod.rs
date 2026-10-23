#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "shared test support fails the test by panicking"
)]
pub mod bot;

use crate::{client, protocol::ServerMessage};
use enco_core::*;
use serde_json::json;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
pub const UNSET_API_KEY_ENV: &str = "ENCO_TEST_UNSET_API_KEY";

pub struct Daemon {
    pub child: Child,
    pub root: tempfile::TempDir,
    pub startup_log: String,
    // Keep the pipe open so the daemon can continue writing diagnostics.
    _stderr: tokio::io::Lines<BufReader<ChildStderr>>,
}

impl Daemon {
    pub async fn start(server: &MockServer, plugin: &str) -> Self {
        Self::start_config(&Self::config(server, plugin)).await
    }

    pub fn config(server: &MockServer, plugin: &str) -> String {
        let base_url = server.uri();
        format!(
            r#"[owner]
timezone = "UTC"

[endpoint.chat]
plugin = {plugin:?}
base_url = {base_url:?}
model = 'mock'
api_key_env = 'ENCO_TEST_API_KEY'
window_tokens = 128000
max_output_tokens = 8192

[profile.default]
reply = ['chat']
compaction = ['chat']

[memory]
embedding = "embed"

[endpoint.embed]
plugin = "openai-compatible"
base_url = {base_url:?}
model = 'embed'
dimensions = 4
"#
        )
    }

    pub async fn start_config(config: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".data")).unwrap();
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        Self::start_home(root).await
    }

    pub async fn start_home(root: tempfile::TempDir) -> Self {
        initialize(root.path()).await;
        let (child, stderr, startup_log) = spawn(root.path(), &[]).await;
        Self {
            child,
            root,
            startup_log,
            _stderr: stderr,
        }
    }

    pub async fn restart(&mut self) {
        self.restart_without(&[]).await;
    }

    pub async fn restart_without(&mut self, variables: &[&str]) {
        let (child, stderr, startup_log) = spawn(self.root.path(), variables).await;
        self.child = child;
        self._stderr = stderr;
        self.startup_log = startup_log;
    }

    pub async fn connect(&self) -> client::Client {
        client::Client::connect(&self.root.path().join(".data/enco.sock"))
            .await
            .unwrap()
    }

    pub async fn stop(&mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
}

pub fn running_generation(plugin: &serde_json::Value) -> &serde_json::Value {
    &plugin["running"]["running"]["activation"]["generation"]
}

pub async fn finish(client: &mut client::Client) -> Vec<Entry> {
    finish_with_timeout(client, Duration::from_secs(30)).await
}

pub async fn finish_with_timeout(client: &mut client::Client, timeout: Duration) -> Vec<Entry> {
    tokio::time::timeout(timeout, async {
        let mut entries = vec![];
        while let Some(line) = client.reader.next_line().await.unwrap() {
            match serde_json::from_str::<ServerMessage>(&line).unwrap() {
                ServerMessage::Entry { entry, .. } => {
                    let done = matches!(entry.body, EntryBody::RunEnded { .. });
                    entries.push(*entry);
                    if done {
                        return entries;
                    }
                }
                ServerMessage::Error { code, message, .. } => panic!("{code}: {message}"),
                _ => {}
            }
        }
        panic!("connection closed before Run ended");
    })
    .await
    .expect("Run did not finish")
}

pub fn response(message: serde_json::Value, reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{ "message": message, "finish_reason": reason }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 2,
            "prompt_cache_hit_tokens": 3,
        },
    }))
}

/// A service-side tool proposal; the host assigns its own CallId when accepting it.
pub fn tool_message(id: &str, name: &str, arguments: serde_json::Value) -> serde_json::Value {
    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": id,
            "type": "function",
            "function": { "name": name, "arguments": arguments.to_string() },
        }],
    })
}

pub async fn embeddings(server: &MockServer) {
    Mock::given(path("/embeddings"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let inputs = body["input"].as_array().unwrap();
            let embeddings: Vec<_> = inputs
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let mut embedding = json!({ "embedding": [1., 0., 0., 1.] });
                    // Gemini omits the zero index. Exercise that wire shape in every memory test.
                    if index != 0 {
                        embedding["index"] = json!(index);
                    }
                    embedding
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({ "data": embeddings }))
        })
        .mount(server)
        .await;
}

async fn spawn(
    root: &Path,
    absent: &[&str],
) -> (Child, tokio::io::Lines<BufReader<ChildStderr>>, String) {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"));
    command
        .arg("serve")
        .env("PATH", without_git(root))
        .env("ENCO_HOME", root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("ENCO_TEST_API_KEY", "enco-test-secret")
        .env_remove(UNSET_API_KEY_ENV)
        .env("ENCO_TEST_SECOND_API_KEY", "enco-second-test-secret")
        .env("ENCO_TEST_TELEGRAM_TOKEN", "42:test-secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for variable in absent {
        command.env_remove(variable);
    }
    let mut child = command.spawn().unwrap();
    let mut startup_log = String::new();
    let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(line) = stderr.next_line().await.unwrap() {
            startup_log.push_str(&line);
            startup_log.push('\n');
            if line.contains("Enco ready") {
                return;
            }
            eprintln!("daemon startup: {line}");
        }
        panic!("daemon ended before accepting connections");
    })
    .await
    .expect("daemon startup timed out");
    (child, stderr, startup_log)
}

pub fn is_reminder(entry: &Entry, id: ScheduleId) -> bool {
    let EntryBody::EventConsumed { event } = &entry.body else {
        return false;
    };
    matches!(event.body, EventBody::Reminder { schedule, .. } if schedule == id)
}

pub async fn initialize(root: &Path) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("init")
        .env("PATH", without_git(root))
        .env("ENCO_HOME", root)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// All daemon scenarios exercise a home without a repository or a usable Git executable.
pub fn without_git(root: &Path) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join("test-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(&git, "#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

/// Each source scenario keeps a warm Cargo cache without locking other builds.
pub fn build_target(root: &Path, scenario: &str) {
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-tests")
        .join(scenario);
    std::fs::create_dir_all(&target).unwrap();
    std::os::unix::fs::symlink(target.canonicalize().unwrap(), root.join(".data/target")).unwrap();
}

pub fn copy_plugin(name: &str, root: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name);
    copy_directory(&source, &root.join("plugins").join(name));
}

pub fn copy_directory(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "target" {
            continue;
        }
        let destination = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}
