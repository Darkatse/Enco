//! Lossless text layout, native images and standalone file uploads.
use crate::{
    channel::{Fault, FileData, Outgoing, Request},
    wire::{self, Conversation},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

const MESSAGE_CHARS: usize = 4000;
/// How long an action carrying a file may take, uploads included.
const FILE_TIMEOUT_MS: u32 = 300_000;

fn image(file: &FileData) -> bool {
    file.data.starts_with(b"\x89PNG\r\n\x1a\n")
        || file.data.starts_with(b"\xff\xd8\xff")
        || (file.data.starts_with(b"RIFF") && file.data.get(8..12) == Some(b"WEBP"))
        || file.data.starts_with(b"GIF87a")
        || file.data.starts_with(b"GIF89a")
}

pub(super) fn split(message: Outgoing) -> Vec<Outgoing> {
    let mut remaining = message.text.as_str();
    let mut parts = vec![];
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
    if let Some(file) = message.file {
        if image(&file)
            && let Some(last) = parts.last_mut()
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

pub(super) fn send(conversation: &str, message: Outgoing) -> Result<Request, Fault> {
    let (key, id, send, upload) = match Conversation::parse(conversation)? {
        Conversation::Private(user) => ("user_id", user, "send_private_msg", "upload_private_file"),
        Conversation::Group(group) => ("group_id", group, "send_group_msg", "upload_group_file"),
    };
    let timeout = if message.file.is_some() {
        FILE_TIMEOUT_MS
    } else {
        wire::ACTION_TIMEOUT_MS
    };
    let mut body = json!({key: id});
    if let Some(file) = &message.file
        && !image(file)
    {
        // An upload has no text field; refuse rather than lose the text.
        if !message.text.is_empty() {
            return Err(wire::invalid("part: a non-image file is sent without text"));
        }
        body["file"] = json!(format!("base64://{}", STANDARD.encode(&file.data)));
        body["name"] = json!(file.name);
        return Ok(wire::request(upload, body, timeout));
    }
    let mut segments = vec![];
    if !message.text.is_empty() {
        segments.push(json!({"type": "text", "data": {"text": message.text}}));
    }
    if let Some(file) = message.file {
        let encoded = format!("base64://{}", STANDARD.encode(&file.data));
        segments.push(json!({"type": "image", "data": {"file": encoded}}));
    }
    body["message"] = json!(segments);
    Ok(wire::request(send, body, timeout))
}
