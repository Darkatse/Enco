#![expect(clippy::unwrap_used, reason = "fixtures fail the test with context")]
use crate::support::channel;
use enco_core::{GenerationId, MappingRecord, Outgoing};
use enco_kernel::{ChannelCallError, Store};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

#[tokio::test]
async fn recorded_channel_calls() {
    if channel::child("channel_mapper::recorded_channel_calls") {
        return;
    }
    let server = MockServer::start().await;
    let (root, store, adapter) = channel::telegram(&server).await;
    let raw = json!({"ok": true, "result": [{"update_id": 12, "message": {
        "message_id": 7, "from": {"id": 9, "first_name": "A"},
        "chat": {"id": 9, "type": "private"}, "text": "hello"
    }}]});
    Mock::given(path("/bot42:test-secret/getUpdates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&raw))
        .mount(&server)
        .await;
    Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"message_id": 88}})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let blobs = root.path().join("blobs");
    let generation = GenerationId(31);
    assert_eq!(adapter.identity().key(), "telegram:42");
    let updates = channel::messages(adapter.clone()).await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].key, "12");
    assert_eq!(updates[0].origin.index, 0);
    let record = mapping(&*store, updates[0].origin.call).await;
    assert_eq!(record.generation, generation);
    assert_eq!(record.export, "channel.received");
    assert_eq!(
        record.arguments["identity"],
        json!({"protocol":"telegram", "account":"42", "facts": "{\"username\":\"FixtureBot\"}"})
    );
    assert_eq!(record.arguments["state"], Value::Null);
    let response = &record.arguments["arrival"]["response"];
    let body = store
        .get_blob(&serde_json::from_value(response["body"].clone()).unwrap())
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), raw);
    let invalid_state = channel::reading(adapter.clone(), Some(Value::Null)).await;
    assert!(
        matches!(invalid_state, Ok(enco_kernel::ChannelReading::Reset {failure, ..}) if failure.code == "channel.invalid_state")
    );
    let sent = adapter
        .send(
            "9",
            &Outgoing {
                text: "hello".into(),
                file: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(sent.message.as_deref(), Some("88"));
    let record = mapping(&*store, sent.origin.call).await;
    assert_eq!(record.export, "channel.answered");
    assert_eq!(record.generation, generation);
    Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_string("protocol drift"))
        .with_priority(1)
        .mount(&server)
        .await;
    let error = adapter
        .send(
            "9",
            &Outgoing {
                text: "again".into(),
                file: None,
            },
        )
        .await;
    assert!(
        matches!(error, Err(ChannelCallError::Protocol(ref fault)) if fault.unknown && fault.origin.is_some())
    );
    for request in server.received_requests().await.unwrap() {
        assert!(request.url.path().starts_with("/bot42:test-secret/"));
    }
    // A failed evidence write is a storage failure, not a normal protocol refusal.
    std::fs::remove_dir_all(&blobs).unwrap();
    std::fs::write(&blobs, b"not a directory").unwrap();
    assert!(matches!(
        channel::reading(adapter.clone(), None).await,
        Err(ChannelCallError::Store(_))
    ));
}

async fn mapping(store: &dyn Store, hash: enco_core::ContentHash) -> MappingRecord {
    let bytes = store.get_blob(&hash).await.unwrap();
    assert!(
        !bytes
            .windows(b"42:test-secret".len())
            .any(|part| part == b"42:test-secret")
    );
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn attachment_exchange_records_each_complete_mapper_input() {
    if channel::child("channel_mapper::attachment_exchange_records_each_complete_mapper_input") {
        return;
    }
    let server = MockServer::start().await;
    Mock::given(path("/gateway/bot42:test-secret/getMe"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"id": 42, "username": "FixtureBot"}})),
        )
        .mount(&server)
        .await;
    let (root, store, adapter) = channel::open(
        channel::TELEGRAM,
        "token",
        &format!("{}/gateway", server.uri()),
    )
    .await;
    Mock::given(path("/gateway/bot42:test-secret/getFile"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"file_path": "images/a.png"}})),
        )
        .mount(&server)
        .await;
    let image = include_bytes!("fixtures/pixel.png");
    Mock::given(path("/gateway/file/bot42:test-secret/images/a.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(image))
        .mount(&server)
        .await;
    let content = adapter.fetch("a-image").await.unwrap();
    assert!(matches!(content, enco_kernel::AttachmentContent::Image(bytes) if bytes == image));
    let mut records = Vec::new();
    for shard in std::fs::read_dir(root.path().join("blobs")).unwrap() {
        for file in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            if let Ok(record) = serde_json::from_slice::<MappingRecord>(&bytes) {
                records.push(record);
            }
        }
    }
    assert_eq!(
        records
            .iter()
            .filter(|record| record.export == "channel.identified")
            .count(),
        1
    );
    let mut calls: Vec<_> = records
        .into_iter()
        .filter(|record| record.export == "channel.fetch")
        .collect();
    calls.sort_by_key(|record| record.arguments["responses"].as_array().unwrap().len());
    assert_eq!(calls.len(), 3);
    for (index, call) in calls.iter().enumerate() {
        assert_eq!(call.generation, GenerationId(31));
        assert_eq!(call.arguments["locator"], "a-image");
        let responses = call.arguments["responses"].as_array().unwrap();
        assert_eq!(responses.len(), index);
        if index == 2 {
            let body = store
                .get_blob(&serde_json::from_value(responses[1]["body"].clone()).unwrap())
                .await
                .unwrap();
            assert_eq!(body, image);
        }
    }
}
