use crate::support::channel;
use enco_core::UnviewableReason;
use enco_kernel::ChannelAction as Action;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

#[tokio::test]
async fn telegram_photos_documents_voice_and_locations_keep_their_message() {
    if channel::child(
        "telegram_images::telegram_photos_documents_voice_and_locations_keep_their_message",
    ) {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    Mock::given(path("/bot42:test-secret/getUpdates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": [
            {"update_id": 1, "message": {"message_id": 1, "chat": {"id": 1, "type": "private"}, "from": {"id": 2, "is_bot": false, "first_name": "Fixture User"}, "caption": "look", "photo": [
                {"file_id": "large", "width": 200, "height": 200},
                {"file_id": "small", "width": 10, "height": 10}
            ]}},
            {"update_id": 2, "message": {"message_id": 2, "chat": {"id": 1, "type": "private"}, "from": {"id": 2, "is_bot": false, "first_name": "Fixture User"}, "document": {
                "file_id": "document", "file_name": "report.pdf", "mime_type": "application/pdf"
            }}},
            {"update_id": 3, "message": {"message_id": 3, "chat": {"id": 1, "type": "private"}, "from": {"id": 2, "is_bot": false, "first_name": "Fixture User"}, "voice": {"file_id": "voice"}}},
            {"update_id": 4, "message": {"message_id": 4, "chat": {"id": 1, "type": "private"}, "from": {"id": 2, "is_bot": false, "first_name": "Fixture User"}, "location": {"latitude": 0, "longitude": 0}}}
        ]}))).expect(1).mount(&server).await;
    let updates = channel::messages(telegram.clone()).await;
    let Action::Message(photo) = &updates[0].input.action else {
        panic!("expected photo")
    };
    assert_eq!(photo.text, "look");
    assert_eq!(photo.attachments.len(), 1);
    assert_eq!(photo.attachments[0].locator, json!("large"));
    let Action::Message(document) = &updates[1].input.action else {
        panic!("expected document")
    };
    assert!(document.text.is_empty());
    assert_eq!(document.attachments[0].locator, json!("document"));
    assert_eq!(document.attachments[0].name, "report.pdf");
    let Action::Message(voice) = &updates[2].input.action else {
        panic!("expected voice")
    };
    assert_eq!(voice.attachments[0].locator, json!("voice"));
    assert_eq!(voice.attachments[0].name, "voice message");
    assert!(voice.unviewable.is_empty());
    let Action::Message(location) = &updates[3].input.action else {
        panic!("expected location")
    };
    assert!(location.attachments.is_empty());
    assert!(matches!(
        location.unviewable[0].reason,
        UnviewableReason::Unsupported
    ));
    // Parsing protocol locations never downloads; the common reader authorizes first.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    Mock::given(path("/bot42:test-secret/getFile"))
        .and(body_json(json!({"file_id": "large"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"file_path": "photos/image.jpg"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/file/bot42:test-secret/photos/image.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"image bytes"))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        telegram
            .fetch_file(&photo.attachments[0].locator)
            .await
            .unwrap()
            .content
            .unwrap(),
        b"image bytes"
    );
}

#[tokio::test]
async fn telegram_attachment_failures_keep_retry_evidence_without_download_urls() {
    if channel::child(
        "telegram_images::telegram_attachment_failures_keep_retry_evidence_without_download_urls",
    ) {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    Mock::given(path("/bot42:test-secret/getFile"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "result": {"file_path": "image.png"}})),
        )
        .mount(&server)
        .await;
    for (status, retryable) in [(403, false), (429, true), (503, true)] {
        let download = Mock::given(path("/file/bot42:test-secret/image.png"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let result = telegram.fetch_file("file-id").await;
        if status == 403 {
            assert!(
                matches!(result.unwrap().content, Err(reason) if reason.contains("403") && !reason.contains("secret"))
            );
            drop(download);
            continue;
        }
        let failure = channel::fault(result);
        assert_eq!(failure.failure.retryable, retryable);
        assert_eq!(failure.failure.code, format!("channel.http.{status}"));
        assert!(!failure.to_string().contains("secret"));
        drop(download);
    }
    // A body over the 32 MiB response limit leaves the file unavailable rather than a fault
    // the host retries.
    let _download = Mock::given(path("/file/bot42:test-secret/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 32 * 1024 * 1024 + 1]))
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    assert!(
        telegram
            .fetch_file("file-id")
            .await
            .unwrap()
            .content
            .is_err()
    );
}
