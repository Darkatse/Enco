//! Attachments are named by what they are and fetched through `getFile`; a file Telegram refuses
//! is unavailable in its own words and the connection stays usable.
use super::*;
use enco_sdk::channel::AttachmentKind;

#[test]
fn all_file_kinds_are_named_attachments_without_mime_classification() -> Result<(), String> {
    for (field, file, name) in [
        (
            "photo",
            json!([{"file_id":"id", "file_unique_id":"unique", "width":640, "height":480}]),
            AttachmentKind::Image.name(),
        ),
        (
            "document",
            json!({"file_id":"id", "file_unique_id":"unique", "file_name":"report.pdf", "mime_type":"application/pdf"}),
            "report.pdf",
        ),
        (
            "document",
            json!({"file_id":"id", "file_unique_id":"unique"}),
            AttachmentKind::File.name(),
        ),
        (
            "document",
            json!({"file_id":"id", "file_unique_id":"unique", "file_name":""}),
            AttachmentKind::File.name(),
        ),
        (
            "document",
            json!({"file_id":"id", "file_unique_id":"unique", "file_name":"picture.png", "mime_type":"application/octet-stream"}),
            "picture.png",
        ),
        (
            "voice",
            json!({"file_id":"id", "file_unique_id":"unique", "duration":2}),
            AttachmentKind::VoiceMessage.name(),
        ),
        (
            "audio",
            json!({"file_id":"id", "file_unique_id":"unique", "duration":2}),
            AttachmentKind::Audio.name(),
        ),
        (
            "audio",
            json!({"file_id":"id", "file_unique_id":"unique", "duration":2, "file_name":"song.mp3"}),
            "song.mp3",
        ),
        (
            "video",
            json!({"file_id":"id", "file_unique_id":"unique", "width":640, "height":480, "duration":2}),
            AttachmentKind::Video.name(),
        ),
        (
            "video_note",
            json!({"file_id":"id", "file_unique_id":"unique", "length":240, "duration":2}),
            AttachmentKind::VideoMessage.name(),
        ),
        (
            "animation",
            json!({"file_id":"id", "file_unique_id":"unique", "width":640, "height":480, "duration":2}),
            AttachmentKind::Animation.name(),
        ),
        (
            "sticker",
            json!({"file_id":"id", "file_unique_id":"unique", "type":"regular", "width":512, "height":512, "is_animated":false, "is_video":false}),
            AttachmentKind::Sticker.name(),
        ),
    ] {
        let mut message = json!({
            "message_id":20, "date":1793361600, "chat":{"id":9,"type":"private"},
            "from":{"id":7,"is_bot":false,"first_name":"Ada"}, "caption":"/clear",
        });
        if field == "animation" {
            // Telegram sets `document` on every animation as well.
            message["document"] = file.clone();
        }
        message[field] = file;
        let Action::Message(content) = receive_message(message)?.action else {
            return Err("caption became command".into());
        };
        assert_eq!(content.attachments.len(), 1);
        assert_eq!(content.attachments[0].name, name);
        assert_eq!(content.attachments[0].locator, "id");
        assert!(content.unviewable.is_empty());
    }
    Ok(())
}

#[test]
fn fetch_drives_two_requests_and_keeps_permanent_file_rejection_local() -> Result<(), String> {
    let FetchStep::Request(first) =
        Plugin::fetch("image-id".into(), vec![], 2).map_err(|fault| fault.failure.message)?
    else {
        return Err("expected getFile".into());
    };
    assert_eq!(first.url, "/bot/getFile");
    let metadata = response(
        200,
        br#"{"ok":true,"result":{"file_path":"photos/a b.jpg"}}"#,
    );
    let FetchStep::Request(download) = Plugin::fetch("image-id".into(), vec![metadata.clone()], 2)
        .map_err(|fault| fault.failure.message)?
    else {
        return Err("expected download".into());
    };
    assert_eq!(download.url, "/file/bot/photos/a%20b.jpg");
    assert_eq!(
        Plugin::fetch(
            "image-id".into(),
            vec![metadata, response(200, b"image bytes")],
            2
        )
        .map_err(|fault| fault.failure.message)?,
        FetchStep::File(b"image bytes".to_vec())
    );
    assert!(matches!(
        Plugin::fetch("missing".into(), vec![response(400, b"not JSON")], 2),
        Ok(FetchStep::Unavailable(_))
    ));
    Ok(())
}

#[test]
fn a_file_telegram_refuses_is_unavailable_in_its_own_words() {
    let refused = response(
        400,
        br#"{"ok":false,"error_code":400,"description":"Bad Request: file is too big"}"#,
    );
    assert!(matches!(
        Plugin::fetch("large".into(), vec![refused], 2),
        Ok(FetchStep::Unavailable(reason)) if reason.contains("file is too big")
    ));
}
