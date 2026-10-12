#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup fails the test with the original context"
)]

use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Output,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::process::Command;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

struct Evaluation {
    root: PathBuf,
    enco: PathBuf,
    judge: MockServer,
    verdict: Arc<AtomicU8>,
    model: MockServer,
}

impl Evaluation {
    async fn new() -> Self {
        let enco = std::env::var_os("ENCO_BIN")
            .map(PathBuf::from)
            .expect("set ENCO_BIN to the built enco binary");
        let root = std::env::temp_dir().join(format!("enco-check-{}", ulid::Ulid::generate()));
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        copy(&fixture, &root.join("evals/tasks/mail"));
        write(
            &root.join("evals/tasks/mail/hidden/mocks/mail.toml"),
            r#"host = "mail.veloran.com"
[[collection]]
name = "inbox"
data = "hidden/inbox.json"
"#,
        );
        write(
            &root.join("evals/tasks/mail/hidden/inbox.json"),
            "[{\"id\":1,\"subject\":\"Meeting {{date:0}}\",\"unread\":true}]",
        );
        let model = MockServer::start().await;
        Mock::given(path("/embeddings")).respond_with(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let count = body["input"].as_array().map_or(1, Vec::len);
            ResponseTemplate::new(200).set_body_json(json!({ "data": (0..count).map(|index| json!({ "index": index, "embedding": [1.0,0.0,0.0,1.0] })).collect::<Vec<_>>() }))
        }).mount(&model).await;
        Mock::given(path("/chat/completions"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let done = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["role"] == "tool");
                let message = if done {
                    json!({ "role": "assistant", "content": "done" })
                } else {
                    json!({ "role": "assistant", "content": null, "tool_calls": [{
                    "id": "read-mail", "type": "function", "function": { "name": "shell_exec",
                    "arguments": json!({ "command": "mail" }).to_string() }
                }] })
                };
                completion(message, if done { "stop" } else { "tool_calls" })
            })
            .mount(&model)
            .await;
        let judge = MockServer::start().await;
        let verdict = Arc::new(AtomicU8::new(0));
        let response = verdict.clone();
        Mock::given(path("/chat/completions")).respond_with(move |_: &wiremock::Request| {
            let word = match response.load(Ordering::SeqCst) { 0 => "PASS", 1 => "UNKNOWN", _ => return ResponseTemplate::new(500) };
            completion(json!({ "role": "assistant", "content": format!("Checked the saved evidence.\n{word}") }), "stop")
        }).mount(&judge).await;
        write(
            &root.join("model.toml"),
            &format!(
                r#"endpoint.chat = {{ plugin = "openai-compatible", base_url = {:?}, model = "agent", window_tokens = 128000, max_output_tokens = 8192 }}
profile.default = {{ reply = "chat", compaction = "chat" }}
memory.embedding = "embed"
endpoint.embed = {{ plugin = "openai-compatible", base_url = {:?}, model = "embedding", dimensions = 4 }}
"#,
                model.uri(),
                model.uri()
            ),
        );
        write(
            &root.join("config.toml"),
            &format!(
                r#"model = "model.toml"
[judge]
base_url = {:?}
model = "judge"
api_key_env = "EVAL_JUDGE_KEY"
"#,
                judge.uri()
            ),
        );
        write(&root.join("variant.toml"), &format!("enco = {:?}\n", enco));
        Self {
            root,
            enco,
            judge,
            verdict,
            model,
        }
    }

    async fn command(&self, args: &[&str], key: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_eval-runner"));
        command
            .args(args)
            .current_dir(&self.root)
            .env_remove("EVAL_JUDGE_KEY");
        if key {
            command.env("EVAL_JUDGE_KEY", "test-credential");
        }
        command.output().await.unwrap()
    }

    async fn run(&self) -> (PathBuf, Value) {
        let output = self
            .command(
                &[
                    "run",
                    "--config",
                    "config.toml",
                    "--variant",
                    "variant.toml",
                    "-k",
                    "1",
                ],
                true,
            )
            .await;
        assert!(
            output.status.success(),
            "{}\n{}\n{}",
            self.root.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let path = PathBuf::from(
            stdout
                .lines()
                .find_map(|line| line.strip_prefix("results: "))
                .unwrap(),
        );
        let result = read(&path);
        (path.parent().unwrap().to_owned(), result)
    }

    /// Point the model under test at another endpoint, keeping the embedding service.
    fn model_at(&self, endpoint: &str) {
        let configuration = self.root.join("model.toml");
        let source = std::fs::read_to_string(&configuration).unwrap();
        write(
            &configuration,
            &source.replacen(&self.model.uri(), endpoint, 1),
        );
    }

    async fn finish(self) {
        tokio::fs::remove_dir_all(&self.root).await.unwrap();
    }
}

fn completion(message: Value, reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "choices": [{ "message": message, "finish_reason": reason }], "usage": { "prompt_tokens": 10, "completion_tokens": 2, "prompt_cache_hit_tokens": 3 } }))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn copy(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test]
async fn evidence_is_isolated_and_regrading_uses_the_saved_trial() {
    use std::os::unix::fs::PermissionsExt;
    let evaluation = Evaluation::new().await;
    let wrapper = evaluation.root.join("enco-wrapper");
    let executable = evaluation.enco.to_str().unwrap().replace('\'', "'\\''");
    write(
        &wrapper,
        &format!(
            "#!/bin/sh\n[ -z \"${{EVAL_JUDGE_KEY+x}}\" ] || exit 42\nexec '{executable}' \"$@\"\n"
        ),
    );
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let artifact = evaluation
        .enco
        .parent()
        .unwrap()
        .join("../factory/openai-compatible.wasm");
    write(
        &evaluation.root.join("variant.toml"),
        &format!(
            "enco = {wrapper:?}\n[[deploy]]\nplugin = \"openai-compatible\"\nartifact = {artifact:?}\n"
        ),
    );
    let (directory, report) = evaluation.run().await;
    let trial = &report["trials"][0];
    assert_eq!(trial["result"], "success", "{trial:#}");
    assert_eq!(trial["network_violations"], 1);
    let trial_dir = directory.join(trial["dir"].as_str().unwrap());
    let facts = read(&trial_dir.join("trial.json"));
    let original = facts["home"].as_str().unwrap();
    assert_eq!(
        std::fs::read_link(trial_dir.join("home/work")).unwrap(),
        Path::new(".enco/workspace")
    );
    assert!(!Path::new(original).parent().unwrap().exists());
    assert!(!trial_dir.join("home/.enco/hidden").exists());
    assert!(!trial_dir.join("home/.enco/task.toml").exists());
    let requests =
        std::fs::read_to_string(trial_dir.join("snapshot/mock/mail/requests.jsonl")).unwrap();
    assert!(requests.contains("/inbox"));
    let plugins = read(&trial_dir.join("snapshot/plugins.json"));
    let active = &plugins
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["name"] == "openai-compatible")
        .unwrap()["active"];
    assert_eq!(trial["generations"]["openai-compatible"], active["id"]);
    assert_eq!(
        report["variants"]["base"]["deploy"][0]["hash"],
        active["artifact"]
    );
    let before = evaluation.judge.received_requests().await.unwrap().len();
    let output = evaluation
        .command(&["regrade", directory.to_str().unwrap()], false)
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        evaluation.judge.received_requests().await.unwrap().len(),
        before
    );
    evaluation.verdict.store(1, Ordering::SeqCst);
    let output = evaluation
        .command(&["regrade", directory.to_str().unwrap(), "--judge"], true)
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        read(&directory.join("result.json"))["trials"][0]["criteria"]["reply"]["result"],
        "fail"
    );
    write(
        &evaluation.root.join("evals/tasks/mail/hidden/calls.json"),
        "{}",
    );
    let output = evaluation
        .command(&["regrade", directory.to_str().unwrap()], false)
        .await;
    assert_eq!(output.status.code(), Some(2));
    evaluation.finish().await;
}

#[tokio::test]
async fn comparison_rejects_missing_samples_and_known_failures_survive_judge_errors() {
    let evaluation = Evaluation::new().await;
    evaluation.verdict.store(2, Ordering::SeqCst);
    let output = evaluation
        .command(
            &[
                "compare",
                "--config",
                "config.toml",
                "--base",
                "variant.toml",
                "--candidate",
                "variant.toml",
                "--tasks",
                "mail",
                "-k",
                "1",
                "--jobs",
                "2",
            ],
            true,
        )
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let result = read(Path::new(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("results: "))
            .unwrap(),
    ));
    assert_eq!(result["comparison"]["verdict"], "reject");
    assert_eq!(result["summary"]["candidate"]["mail"]["valid"], 0);
    write(
        &evaluation.root.join("evals/tasks/mail/hidden/calls.json"),
        "false",
    );
    let (_, result) = evaluation.run().await;
    assert_eq!(result["trials"][0]["result"], "failure");
    assert_eq!(result["trials"][0]["criteria"]["reply"]["result"], "error");
    evaluation.finish().await;
}

#[tokio::test]
async fn trials_past_their_deadline_cancel_the_run_and_fail() {
    let evaluation = Evaluation::new().await;
    write(
        &evaluation.root.join("evals/tasks/mail/hidden/calls.json"),
        "{}",
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    evaluation.model_at(&format!("http://{}", listener.local_addr().unwrap()));
    let task = evaluation.root.join("evals/tasks/mail/task.toml");
    let source = std::fs::read_to_string(&task).unwrap();
    write(
        &task,
        &source.replace("timeout_secs = 30", "timeout_secs = 3"),
    );
    // This endpoint never responds. Its socket closes when Enco settles cancellation.
    let mut stalled = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        tokio::io::copy(&mut stream, &mut tokio::io::sink())
            .await
            .unwrap();
    });
    let (directory, report) = evaluation.run().await;
    let received = tokio::time::timeout(std::time::Duration::from_secs(1), &mut stalled).await;
    if received.is_err() {
        stalled.abort();
        let _ = stalled.await;
    }
    received
        .expect("provider was not reached before the trial deadline")
        .unwrap();
    let trial = &report["trials"][0];
    assert_eq!(trial["result"], "failure");
    let log = read(
        &directory
            .join(trial["dir"].as_str().unwrap())
            .join("snapshot/log/main.json"),
    );
    assert_eq!(
        log.as_array().unwrap().last().unwrap()["body"]["end"]["kind"],
        "cancelled"
    );
    let output = evaluation
        .command(&["regrade", directory.to_str().unwrap()], false)
        .await;
    assert!(output.status.success());
    assert_eq!(
        read(&directory.join("result.json"))["trials"][0]["result"],
        "failure"
    );
    evaluation.finish().await;
}

#[tokio::test]
async fn unreachable_providers_remain_execution_failures() {
    let evaluation = Evaluation::new().await;
    write(
        &evaluation.root.join("evals/tasks/mail/hidden/calls.json"),
        "{}",
    );
    // Nothing listens on the port once the probe listener is dropped.
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    evaluation.model_at(&format!("http://{address}"));
    let (_, report) = evaluation.run().await;
    let trial = &report["trials"][0];
    assert_eq!(trial["result"], "failure");
    assert!(
        trial["reason"]
            .as_str()
            .unwrap()
            .contains("provider.network"),
        "{trial}"
    );
    evaluation.finish().await;
}
