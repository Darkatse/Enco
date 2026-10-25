//! Sending a reply to Telegram: splitting long text at line breaks and attaching files.
use super::{Body, Fault, Telegram, invalid_response};
use crate::limits::{
    TELEGRAM_MESSAGE_CHARS, TELEGRAM_PHOTO_MAX_ASPECT_RATIO, TELEGRAM_PHOTO_MAX_BYTES,
    TELEGRAM_PHOTO_MAX_DIMENSION_SUM, TELEGRAM_SEND_SECONDS, TELEGRAM_UPLOAD_SECONDS,
};
use enco_core::{FileData, ImageFormat, Outgoing};
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use std::path::Path;

fn media_kind(file: &FileData) -> &'static str {
    if ImageFormat::detect(&file.data).is_some() {
        let photo = file.data.len() <= TELEGRAM_PHOTO_MAX_BYTES
            && imagesize::blob_size(&file.data).is_ok_and(|size| {
                let (long, short) = (size.width.max(size.height), size.width.min(size.height));
                size.width + size.height <= TELEGRAM_PHOTO_MAX_DIMENSION_SUM
                    && long <= short * TELEGRAM_PHOTO_MAX_ASPECT_RATIO
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

pub(super) fn split(message: Outgoing<FileData>) -> Vec<Outgoing<FileData>> {
    let Outgoing { text, file } = message;
    let mut remaining = text.as_str();
    let mut parts = Vec::new();
    while !remaining.is_empty() {
        let mut end = remaining
            .char_indices()
            .nth(TELEGRAM_MESSAGE_CHARS)
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
            && last.text.chars().count() + link <= TELEGRAM_MESSAGE_CHARS
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

pub(super) async fn send(
    telegram: &Telegram,
    target: &str,
    message: &Outgoing<FileData>,
) -> Result<(), Fault> {
    let (body, timeout) = match &message.file {
        None => (
            Body::Json(json!({"chat_id": target, "rich_message": {"markdown": message.text}})),
            TELEGRAM_SEND_SECONDS,
        ),
        Some(file) => {
            let kind = media_kind(file);
            let link = media_link(kind);
            let markdown = if message.text.is_empty() {
                link
            } else {
                format!("{}\n\n{link}", message.text)
            };
            let rich_message = json!({
                "markdown": markdown,
                "media": [{"id": "file", "media": {"type": kind, "media": "attach://file"}}]
            });
            let form = Form::new()
                .text("chat_id", target.to_owned())
                .text("rich_message", rich_message.to_string())
                .part(
                    "file",
                    Part::bytes(file.data.clone()).file_name(file.name.clone()),
                );
            (Body::Multipart(form), TELEGRAM_UPLOAD_SECONDS)
        }
    };
    let value = telegram.request("sendRichMessage", body, timeout).await?;
    if value.get("message_id").and_then(Value::as_i64).is_none() {
        return Err(invalid_response("sendRichMessage"));
    }
    Ok(())
}
