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
async fn a_reminder_created_by_the_model_is_delivered_once_after_daemon_restart() {
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
            let at = (Utc::now() + chrono::Duration::seconds(2)).to_rfc3339();
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
    let schedules: Vec<Schedule> =
        serde_json::from_value(client.request(Command::Schedules {}).await.unwrap()).unwrap();
    assert_eq!(schedules.len(), 1);
    let reminder = &schedules[0];
    daemon.stop().await;
    // Wait for the actual due time, not an arbitrary delay used as a readiness assertion.
    let remaining = (reminder.due_at + chrono::Duration::seconds(1) - Utc::now())
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep_until(tokio::time::Instant::now() + remaining).await;
    daemon.restart().await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    let log: Vec<Entry> = serde_json::from_value(
        client
            .request(Command::Log {
                session: "main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    let completed = log
        .iter()
        .filter(|e| matches!(e.body, EntryBody::RunEnded { .. }))
        .count();
    if completed < 2 {
        finish(&mut client).await;
    }
    let log: Vec<Entry> = serde_json::from_value(
        client
            .request(Command::Log {
                session: "main".into(),
                after: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        log.iter()
            .filter(|entry| is_reminder(entry, reminder.id))
            .count(),
        1
    );
    assert!(
        client
            .request(Command::Schedules {})
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        client
            .request(Command::CancelSchedule {
                schedule_id: reminder.id
            })
            .await
            .unwrap()["cancelled"],
        false
    );
    daemon.stop().await;
}
