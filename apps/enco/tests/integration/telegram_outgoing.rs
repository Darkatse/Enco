#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support::channel;
use enco_core::{FileData, Outgoing};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes
}

fn message(name: &str, data: Vec<u8>) -> Outgoing<FileData> {
    Outgoing {
        text: "**Progress**".into(),
        file: Some(FileData {
            name: name.into(),
            data,
        }),
    }
}

fn rich_message(body: &[u8]) -> Value {
    let body = String::from_utf8_lossy(body);
    let field = body.split("name=\"rich_message\"").nth(1).unwrap();
    let value = field.split("\r\n\r\n").nth(1).unwrap();
    serde_json::from_str(value.split("\r\n--").next().unwrap()).unwrap()
}

#[tokio::test]
async fn telegram_uploads_native_media_and_preserves_other_files() {
    if channel::child("telegram_outgoing::telegram_uploads_native_media_and_preserves_other_files")
    {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "result": {"message_id": 1}
        })))
        .mount(&server)
        .await;
    let mut oversized = png(100, 100);
    oversized.resize(10_000_001, 0);
    for (name, bytes, kind) in [
        ("screenshot.png", png(100, 100), "photo"),
        ("recording.MP3", b"mp3 bytes".to_vec(), "audio"),
        ("recording.m4a", b"m4a bytes".to_vec(), "audio"),
        ("clip.mp4", b"mp4 bytes".to_vec(), "video"),
        ("report.pdf", b"pdf bytes".to_vec(), "document"),
        ("animation.gif", b"GIF89a".to_vec(), "document"),
        ("large.png", oversized, "document"),
        ("long.png", png(100, 2001), "document"),
        ("wide.png", png(6000, 5000), "document"),
        ("broken.png", b"\x89PNG\r\n\x1a\n".to_vec(), "document"),
    ] {
        let outgoing = message(name, bytes);
        telegram.send("7", &outgoing).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        let request = requests.last().unwrap();
        assert!(
            request.headers["content-type"]
                .to_str()
                .unwrap()
                .starts_with("multipart/form-data;")
        );
        let rich = rich_message(&request.body);
        assert_eq!(
            rich["markdown"],
            format!("**Progress**\n\n![](tg://{kind}?id=file)")
        );
        assert_eq!(
            rich["media"],
            json!([{"id": "file", "media": {"type": kind, "media": "attach://file"}}])
        );
        let file = outgoing.file.unwrap();
        assert!(
            request
                .body
                .windows(file.data.len())
                .any(|window| window == file.data)
        );
        assert!(String::from_utf8_lossy(&request.body).contains(&format!("filename=\"{name}\"")));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 11);
}

#[tokio::test]
async fn telegram_splits_text_without_copying_the_attachment_into_earlier_parts() {
    if channel::child(
        "telegram_outgoing::telegram_splits_text_without_copying_the_attachment_into_earlier_parts",
    ) {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "result": {"message_id": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;
    for text in [
        String::new(),
        "汉".repeat(32768),
        "🌱".repeat(32769),
        "x".repeat(32740),
    ] {
        let mut outgoing = message("a.pdf", b"file".to_vec());
        outgoing.text = text.clone();
        let parts = telegram.split(outgoing).await.unwrap();
        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(parts.iter().all(|part| part.text.chars().count() <= 32768));
        assert_eq!(parts.iter().filter(|part| part.file.is_some()).count(), 1);
        assert!(parts.last().unwrap().file.is_some());
        if text.is_empty() {
            assert_eq!(parts.len(), 1);
            telegram.send("7", &parts[0]).await.unwrap();
        } else if text.chars().count() == 32768 {
            assert_eq!(parts.len(), 2);
            assert!(parts[1].text.is_empty());
        }
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        rich_message(&requests[1].body)["markdown"],
        "![](tg://document?id=file)"
    );
}

#[tokio::test]
async fn telegram_uploads_share_http_failure_classification() {
    if channel::child("telegram_outgoing::telegram_uploads_share_http_failure_classification") {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    for (status, unknown) in [(403, false), (500, true)] {
        let guard = Mock::given(path("/bot42:test-secret/sendRichMessage"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let fault = channel::fault(
            telegram
                .send("7", &message("a.pdf", b"file".to_vec()))
                .await,
        );
        assert_eq!(fault.unknown, unknown);
        assert_eq!(fault.failure.code, format!("channel.http.{status}"));
        assert!(!fault.to_string().contains("secret"));
        drop(guard);
    }
}

#[tokio::test]
async fn telegram_upload_timeout_after_receipt_is_unknown() {
    if channel::child("telegram_outgoing::telegram_upload_timeout_after_receipt_is_unknown") {
        return;
    }
    let server = MockServer::start().await;
    let (_root, _store, telegram) = channel::telegram(&server).await;
    let (received, receipt) = tokio::sync::oneshot::channel();
    let received = std::sync::Mutex::new(Some(received));
    Mock::given(path("/bot42:test-secret/sendRichMessage"))
        .respond_with(move |_request: &wiremock::Request| {
            received.lock().unwrap().take().unwrap().send(()).unwrap();
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(130))
        })
        .mount(&server)
        .await;
    let send = tokio::spawn(async move {
        telegram
            .send("7", &message("a.pdf", b"file".to_vec()))
            .await
    });
    receipt.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(121)).await;
    let fault = channel::fault(send.await.unwrap());
    assert!(fault.unknown);
    assert_eq!(fault.failure.code, "channel.network");
    assert!(!fault.to_string().contains("test-secret"));
}
