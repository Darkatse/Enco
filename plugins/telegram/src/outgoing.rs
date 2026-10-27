//! Rich messages, lossless splitting and deterministic multipart uploads.
use crate::{
    channel::{FileData, Outgoing, Request},
    limits::{
        MESSAGE_CHARS, PHOTO_MAX_ASPECT_RATIO, PHOTO_MAX_BYTES, PHOTO_MAX_DIMENSION_SUM,
        SEND_TIMEOUT_MS, UPLOAD_TIMEOUT_MS,
    },
    wire::request,
};
use enco_sdk::channel::{FormPart, multipart};
use serde_json::json;
use std::path::Path;

fn media_kind(file: &FileData) -> &'static str {
    let supported = file.data.starts_with(b"\x89PNG\r\n\x1a\n")
        || file.data.starts_with(b"\xff\xd8\xff")
        || (file.data.starts_with(b"RIFF") && file.data.get(8..12) == Some(b"WEBP"));
    if supported {
        let photo = file.data.len() <= PHOTO_MAX_BYTES
            && imagesize::blob_size(&file.data).is_ok_and(|size| {
                let (width, height) = (size.width as u64, size.height as u64);
                let (long, short) = (width.max(height), width.min(height));
                width + height <= PHOTO_MAX_DIMENSION_SUM && long <= short * PHOTO_MAX_ASPECT_RATIO
            });
        return if photo { "photo" } else { "document" };
    }
    match Path::new(&file.name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3" | "m4a") => "audio",
        Some("mp4") => "video",
        _ => "document",
    }
}

fn media_link(kind: &str) -> String {
    format!("![](tg://{kind}?id=file)")
}

pub(super) fn split(message: Outgoing) -> Vec<Outgoing> {
    let Outgoing { text, file } = message;
    let mut remaining = text.as_str();
    let mut parts = Vec::new();
    while !remaining.is_empty() {
        let mut end = remaining
            .char_indices()
            .nth(MESSAGE_CHARS)
            .map_or(remaining.len(), |(index, _)| index);
        if end < remaining.len() {
            end = remaining[..end].rfind('\n').map_or(end, |index| index + 1);
        }
        parts.push(Outgoing {
            text: remaining[..end].into(),
            file: None,
        });
        remaining = &remaining[end..];
    }
    if let Some(file) = file {
        // Room for the longest media link and its separating blank line.
        let link = media_link("document").chars().count() + 2;
        if let Some(last) = parts.last_mut()
            && last.text.chars().count() + link <= MESSAGE_CHARS
        {
            last.file = Some(file);
        } else {
            parts.push(Outgoing {
                text: String::new(),
                file: Some(file),
            });
        }
    }
    parts
}

pub(super) fn send(conversation: String, message: Outgoing) -> Request {
    let Some(file) = message.file else {
        return request(
            "sendRichMessage",
            json!({"chat_id": conversation, "rich_message": {"markdown": message.text}}),
            SEND_TIMEOUT_MS,
        );
    };
    let kind = media_kind(&file);
    let link = media_link(kind);
    let markdown = if message.text.is_empty() {
        link
    } else {
        format!("{}\n\n{link}", message.text)
    };
    let media = json!({"id": "file", "media": {"type": kind, "media": "attach://file"}});
    let rich_message = json!({"markdown": markdown, "media": [media]}).to_string();
    let (content_type, body) = multipart(&[
        FormPart {
            name: "chat_id",
            filename: None,
            content_type: None,
            data: conversation.as_bytes(),
        },
        FormPart {
            name: "rich_message",
            filename: None,
            content_type: None,
            data: rich_message.as_bytes(),
        },
        FormPart {
            name: "file",
            filename: Some(&file.name),
            content_type: Some("application/octet-stream"),
            data: &file.data,
        },
    ]);
    Request {
        method: "POST".into(),
        url: "/bot/sendRichMessage".into(),
        headers: vec![("content-type".into(), content_type)],
        body: Some(body),
        timeout_ms: Some(UPLOAD_TIMEOUT_MS),
    }
}
