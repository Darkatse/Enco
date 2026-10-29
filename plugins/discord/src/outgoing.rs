//! Components V2 rendering and bounded multipart messages, including the Run's activity view.
use crate::{
    channel::{Outgoing, Request},
    wire,
};
use enco_sdk::channel::{ActivityView, CANCEL_LABEL, FormPart, multipart};
use serde_json::{Value, json};

const TEXT_LIMIT: usize = 4000;
const COMPONENTS_V2: u32 = 1 << 15;
/// Discord's limit on the components of one message, nested ones included.
const MAX_COMPONENTS: usize = 40;
/// The dividers one part may hold: each can add a text display and a separator, and the
/// container, trailing text and attachment take one component each.
const MAX_DIVIDERS: usize = (MAX_COMPONENTS - 3) / 2;
/// Discord's blurple, the accent of every message's container.
const ACCENT_COLOR: u32 = 0x5865F2;
const UPLOAD_TIMEOUT_MS: u32 = 300_000;

fn separator(line: &str) -> bool {
    matches!(line.trim(), "---" | "***" | "___")
}

fn table_divider(line: &str) -> bool {
    line.contains('|')
        && line.trim().trim_matches('|').split('|').all(|cell| {
            let cell = cell.trim();
            cell.contains('-') && cell.bytes().all(|byte| matches!(byte, b'-' | b':'))
        })
}

fn tables(text: &str) -> String {
    let mut rendered = String::new();
    let mut lines = text.split_inclusive('\n').peekable();
    let mut table = false;
    let mut code = false;
    while let Some(line) = lines.next() {
        let fence = line.trim_start().starts_with("```");
        let row = !code
            && !fence
            && line.contains('|')
            && (table || lines.peek().is_some_and(|line| table_divider(line)));
        if row != table {
            rendered.push_str("```\n");
            table = row;
        }
        rendered.push_str(line);
        if fence {
            code = !code;
        }
    }
    if table {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str("```");
    }
    rendered
}

pub(super) fn split(message: Outgoing) -> Vec<Outgoing> {
    // Parts break between lines regardless of code fences and tables, so a block longer than
    // a part is rendered in pieces that may lose its formatting.
    let mut rest = message.text.as_str();
    let mut parts = Vec::new();
    while !rest.is_empty() {
        let mut units = 0;
        let mut end = rest.len();
        for (index, character) in rest.char_indices() {
            units += character.len_utf16();
            if units > TEXT_LIMIT {
                end = index;
                break;
            }
        }
        if end < rest.len() {
            end = rest[..end].rfind('\n').map_or(end, |index| index + 1);
        }
        let mut separators = 0;
        for (index, line) in rest[..end].split_inclusive('\n').scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, line))
        }) {
            if separator(line) {
                separators += 1;
            }
            if separators == MAX_DIVIDERS {
                end = index + line.len();
                break;
            }
        }
        loop {
            let units = tables(&rest[..end]).encode_utf16().count();
            if units <= TEXT_LIMIT {
                break;
            }
            let mut excess = units - TEXT_LIMIT;
            for (index, character) in rest[..end].char_indices().rev() {
                if index == 0 {
                    break;
                }
                end = index;
                excess = excess.saturating_sub(character.len_utf16());
                if excess == 0 {
                    break;
                }
            }
        }
        parts.push(Outgoing {
            text: rest[..end].into(),
            file: None,
        });
        rest = &rest[end..];
    }
    if let Some(file) = message.file {
        if let Some(last) = parts.last_mut() {
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

fn text_components(text: &str) -> Vec<Value> {
    let mut components = Vec::new();
    let mut body = String::new();
    let mut code = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            code = !code;
        }
        if !code && separator(line) {
            if !body.is_empty() {
                components.push(json!({"type": 10, "content": std::mem::take(&mut body)}));
            }
            components.push(json!({"type": 14, "divider": true, "spacing": 1}));
        } else {
            body.push_str(line);
        }
    }
    if !body.is_empty() {
        components.push(json!({"type": 10, "content": body}));
    }
    components
}

/// `text` shown as written in Discord's Markdown, as tool arguments may contain its inline syntax:
/// emphasis, code, spoilers, mentions and masked links.
fn literal(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '*' | '_' | '~' | '`' | '|' | '<' | '[') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// A message body showing `components` in the accent container, mentioning no one.
fn components_v2(components: Vec<Value>) -> Value {
    let container = json!({"type": 17, "accent_color": ACCENT_COLOR, "components": components});
    json!({
        "flags": COMPONENTS_V2,
        "allowed_mentions": {"parse": []},
        "components": [container],
    })
}

pub(super) fn send(conversation: String, message: Outgoing) -> Request {
    let mut components = text_components(&tables(&message.text));
    if let Some(file) = &message.file {
        let image = file.data.starts_with(b"\x89PNG\r\n\x1a\n")
            || file.data.starts_with(b"\xff\xd8\xff")
            || file.data.starts_with(b"GIF8")
            || (file.data.starts_with(b"RIFF") && file.data.get(8..12) == Some(b"WEBP"));
        let attachment = format!("attachment://{}", file.name);
        components.push(if image {
            json!({"type": 12, "items": [{"media": {"url": attachment}}]})
        } else {
            json!({"type": 13, "file": {"url": attachment}})
        });
    }
    let mut payload = components_v2(components);
    let url = format!("/channels/{}/messages", wire::path(&conversation));
    let Some(file) = message.file else {
        return wire::request("POST", url, Some(payload));
    };
    payload["attachments"] = json!([{"id": 0, "filename": file.name}]);
    let payload = payload.to_string();
    let (content_type, body) = multipart(&[
        FormPart {
            name: "payload_json",
            filename: None,
            content_type: Some("application/json"),
            data: payload.as_bytes(),
        },
        FormPart {
            name: "files[0]",
            filename: Some(&file.name),
            content_type: Some("application/octet-stream"),
            data: &file.data,
        },
    ]);
    Request {
        method: "POST".into(),
        url,
        headers: vec![("content-type".into(), content_type)],
        body: Some(body),
        timeout_ms: Some(UPLOAD_TIMEOUT_MS),
    }
}

pub(super) fn activity(
    conversation: String,
    shown: Option<String>,
    activity: Option<ActivityView>,
) -> Option<Request> {
    let url = format!("/channels/{}/messages", wire::path(&conversation));
    match (shown, activity) {
        (Some(shown), None) => Some(wire::request(
            "DELETE",
            format!("{url}/{}", wire::path(&shown)),
            None,
        )),
        (shown, Some(activity)) => {
            let cancel = json!({
                "type": 2,
                "style": 4,
                "label": CANCEL_LABEL,
                "custom_id": "cancel",
            });
            let body = components_v2(vec![
                json!({"type": 10, "content": literal(&activity.text())}),
                json!({"type": 1, "components": [cancel]}),
            ]);
            Some(match shown {
                Some(shown) => {
                    wire::request("PATCH", format!("{url}/{}", wire::path(&shown)), Some(body))
                }
                None => wire::request("POST", url, Some(body)),
            })
        }
        (None, None) => None,
    }
}
