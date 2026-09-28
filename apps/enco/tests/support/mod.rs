#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use crate::{client, protocol::ServerMessage};
use enco_core::*;
use serde_json::json;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
pub struct Daemon {
    pub child: Child,
    pub root: tempfile::TempDir,
    // Keep the pipe open so the daemon can continue writing diagnostics.
    _stderr: tokio::io::Lines<BufReader<ChildStderr>>,
}

impl Daemon {
    pub async fn start(server: &MockServer, plugin: &str) -> Self {
        let base_url = server.uri();
        let config = format!(
            r#"[provider]
plugin = {plugin:?}
base_url = {base_url:?}
model = 'mock'

[embedding]
base_url = {base_url:?}
model = 'embed'
dimensions = 4
"#
        );
        Self::start_config(&config).await
    }

    pub async fn start_config(config: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        let (child, stderr) = spawn(root.path()).await;
        Self {
            child,
            root,
            _stderr: stderr,
        }
    }

    pub async fn restart(&mut self) {
        let (child, stderr) = spawn(self.root.path()).await;
        self.child = child;
        self._stderr = stderr;
    }

    pub async fn connect(&self) -> client::Client {
        client::Client::connect(&self.root.path().join("enco.sock"))
            .await
            .unwrap()
    }

    pub async fn stop(&mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
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
                    entries.push(entry);
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

async fn spawn(root: &Path) -> (Child, tokio::io::Lines<BufReader<ChildStderr>>) {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("serve")
        .env("ENCO_HOME", root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(line) = stderr.next_line().await.unwrap() {
            if line.contains("Enco ready") {
                return;
            }
            eprintln!("daemon startup: {line}");
        }
        panic!("daemon ended before accepting connections");
    })
    .await
    .expect("daemon startup timed out");
    (child, stderr)
}

pub fn is_reminder(entry: &Entry, id: ScheduleId) -> bool {
    let EntryBody::EventConsumed { event } = &entry.body else {
        return false;
    };
    matches!(event.body, EventBody::Reminder { schedule, .. } if schedule == id)
}
