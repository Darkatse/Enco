#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../src/client.rs"]
mod client;

#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::*;
use protocol::Command;
use serde_json::json;
use support::*;
use wiremock::{Mock, MockServer, matchers::path};

#[tokio::test]
async fn wasm_memory_write_is_visible_to_native_admin_and_the_next_request() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.iter().any(|message| message["role"] == "tool") {
                return response(json!({ "role": "assistant", "content": "saved" }), "stop");
            }
            let message = tool_message(
                "remember-owner",
                "memory_save",
                json!({ "text": "Owner is River", "pinned": true }),
            );
            response(message, "tool_calls")
        })
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "deepseek").await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            session: "main".into(),
            text: "remember my name".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let entries = finish(&mut client).await;
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("memory")
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let memory: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(memory["memories"][0]["text"], "Owner is River");
    assert!(memory["unindexed"].as_array().unwrap().is_empty());
    let requests = server.received_requests().await.unwrap();
    let chat: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/chat/completions")
        .collect();
    let body: serde_json::Value = serde_json::from_slice(&chat[1].body).unwrap();
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Owner is River")
    );
    assert!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/embeddings")
            .any(
                |r| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap()["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t == "Owner is River")
            )
    );
    let id: MemoryId = serde_json::from_value(memory["memories"][0]["id"].clone()).unwrap();
    assert_eq!(
        client
            .request(Command::ForgetMemory { memory_id: id })
            .await
            .unwrap()["forgotten"],
        true
    );
    assert!(
        client.request(Command::Memories {}).await.unwrap()["memories"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    daemon.stop().await;
}
