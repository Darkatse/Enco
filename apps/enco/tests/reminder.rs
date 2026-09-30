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
async fn model_created_reminders_are_listed_and_cancelled_through_the_cli() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.iter().any(|message| message["role"] == "tool") {
                return response(
                    json!({ "role": "assistant", "content": "reminder accepted" }),
                    "stop",
                );
            }
            let at = (Utc::now() + std::time::Duration::from_secs(3600)).to_rfc3339();
            let message = tool_message(
                "remind-once",
                "schedule_create",
                json!({ "at": at, "message": "check the kettle" }),
            );
            response(message, "tool_calls")
        })
        .mount(&server)
        .await;
    let mut daemon = Daemon::start(&server, "openai-compatible").await;
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
            text: "remind me".into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    finish(&mut client).await;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .arg("schedules")
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let schedules: Vec<Schedule> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schedules.len(), 1);
    let reminder = &schedules[0];
    assert_eq!(reminder.message, "check the kettle");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args(["schedules", "--cancel", &reminder.id.to_string()])
        .env("ENCO_HOME", daemon.root.path())
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let cancelled: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cancelled["cancelled"], true);
    assert!(
        client
            .request(Command::Schedules {})
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    daemon.stop().await;
}
