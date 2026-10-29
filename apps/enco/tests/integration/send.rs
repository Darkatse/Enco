use enco_core::*;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn waiting_for_an_input_survives_pushes_that_overtake_a_catch_up() {
    let root = tempfile::tempdir().unwrap();
    tokio::fs::create_dir(root.path().join(".data"))
        .await
        .unwrap();
    let listener = tokio::net::UnixListener::bind(root.path().join(".data/enco.sock")).unwrap();
    let session = SessionId::new();
    let run = RunId::new();
    let directory = root.path().canonicalize().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader).lines();
        let request: Value =
            serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
        writer
            .write_all(
                format!(
                    "{}\n",
                    json!({ "type": "ok", "id": request["id"], "data": { "session_id": session } })
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let request: Value =
            serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["text"], "");
        assert_eq!(
            request["images"],
            json!([directory.join("first.png"), directory.join("second.webp")])
        );
        let event_id: EventId = serde_json::from_value(request["event_id"].clone()).unwrap();
        let send_id = request["id"].clone();
        let [consumed, ended] = run_entries(session, event_id, run);
        // The broadcast missed `consumed` and resumes with retained entries before the
        // catch-up request arrives; a pushed entry must not hide the gap.
        writer
            .write_all(
                format!(
                    "{}\n{}\n{}\n{}\n",
                    json!({ "type": "error", "id": 0, "code": "lagged", "message": "resync" }),
                    json!({ "type": "ok", "id": send_id, "data": { "session_id": session, "event_id": event_id, "accepted": "new" } }),
                    json!({ "type": "entry", "session": "main", "entry": ended }),
                    json!({ "type": "error", "id": 0, "code": "lagged", "message": "resync again" }),
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let request: Value =
            serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["cmd"], "log");
        writer
            .write_all(
                format!(
                    "{}\n",
                    json!({ "type": "ok", "id": request["id"], "data": [consumed, ended] })
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        assert!(reader.next_line().await.unwrap().is_none());
        event_id
    });
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_enco"))
        .args([
            "send",
            "--wait",
            "--image",
            "first.png",
            "--image",
            "second.webp",
        ])
        .current_dir(root.path())
        .env("ENCO_HOME", root.path())
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["event_id"], json!(server.await.unwrap()));
    assert_eq!(result["run"], json!(run));
}

fn run_entries(session: SessionId, event_id: EventId, run: RunId) -> [Entry; 2] {
    let consumed = Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(10),
        },
        at: Utc::now(),
        body: EntryBody::EventConsumed {
            event: Event {
                id: event_id,
                session,
                source: EventSource::Cli,
                body: EventBody::UserMessage {
                    quote: None,
                    images: Vec::new(),
                    unviewable: Vec::new(),
                    earlier: Vec::new(),
                    text: "second input".into(),
                },
                received_at: Utc::now(),
            },
        },
    };
    let ended = Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(12),
        },
        at: Utc::now(),
        body: EntryBody::RunEnded {
            run,
            end: RunEnd::Completed,
        },
    };
    [consumed, ended]
}
