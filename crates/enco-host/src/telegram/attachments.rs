use super::{Fault, Telegram, http_error, invalid_response, network, parse_action};
use crate::channel::{Action, Attachment, MessageContent};
use enco_core::{Unviewable, UnviewableReason};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
pub(super) struct Content {
    text: Option<String>,
    caption: Option<String>,
    #[serde(default)]
    photo: Vec<Photo>,
    document: Option<Document>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

#[derive(Deserialize)]
struct Photo {
    file_id: String,
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
struct Document {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

impl Content {
    pub(super) fn action(self) -> Option<Action> {
        let mut message = MessageContent {
            text: self.text.clone().or(self.caption).unwrap_or_default(),
            images: Vec::new(),
            unviewable: Vec::new(),
        };
        if let Some(photo) = self
            .photo
            .into_iter()
            .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height))
        {
            message.images.push(Attachment {
                what: "photo".into(),
                locator: photo.file_id,
            });
        }
        if let Some(document) = self.document {
            let mime = document.mime_type.as_deref().unwrap_or("unknown MIME type");
            let name = document.file_name.as_deref().unwrap_or("unnamed");
            let what = format!("file {name} ({mime})");
            if mime.starts_with("image/") {
                message.images.push(Attachment {
                    what,
                    locator: document.file_id,
                });
            } else {
                message.unviewable.push(Unviewable {
                    what,
                    reason: UnviewableReason::Unsupported,
                });
            }
        }
        for kind in [
            "voice",
            "audio",
            "video",
            "video_note",
            "animation",
            "sticker",
            "contact",
            "location",
            "venue",
            "poll",
            "story",
            "paid_media",
        ] {
            if self.other.contains_key(kind) {
                message.unviewable.push(Unviewable {
                    what: format!("{kind} message"),
                    reason: UnviewableReason::Unsupported,
                });
            }
        }
        if !message.images.is_empty() || !message.unviewable.is_empty() {
            Some(Action::Message(message))
        } else {
            self.text.map(parse_action)
        }
    }
}

pub(super) async fn fetch(telegram: &Telegram, file_id: &str) -> Result<Vec<u8>, Fault> {
    let file = telegram
        .request("getFile", json!({"file_id": file_id}), 30)
        .await?;
    let path = file
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("getFile"))?;
    let response = telegram
        .client
        .get(format!(
            "{}/file/bot{}/{}",
            telegram.base, telegram.token, path
        ))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| network("download file", error))?;
    if !response.status().is_success() {
        return Err(http_error(
            "download file",
            response.status().as_u16(),
            None,
        ));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| network("download file", error))
}
