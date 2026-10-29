//! Chooses whole image entries newest first and describes every owner attachment.
use crate::composer::*;
use crate::{key, limits::*, tokens};
use enco_sdk::types::{Unviewable, UnviewableReason};
use std::collections::BTreeSet;

pub(super) fn selected(input: &ComposeInput, omitted: &mut Vec<Omission>) -> BTreeSet<(u64, u64)> {
    let accepts = input.reply.accepts.contains(&Modality::Image);
    let mut remaining = REQUEST_IMAGE_MAX_BYTES;
    let mut selected = BTreeSet::new();
    for item in input.transcript.items.iter().rev() {
        let images: Vec<_> = item
            .message
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Image(image) => Some(image),
                _ => None,
            })
            .collect();
        if images.is_empty() {
            continue;
        }
        let size: u64 = images.iter().map(|image| image.size).sum();
        if accepts && size <= remaining {
            remaining -= size;
            selected.insert(key(&item.pos));
        } else {
            remaining = 0;
            for image in images {
                omitted.push(Omission {
                    source: format!("image:{}:{}:{}", item.pos.epoch, item.pos.seq, image.content),
                    reason: if accepts {
                        format!("entry's images do not fit the {REQUEST_IMAGE_MAX_BYTES}-byte image budget")
                    } else { "reply endpoint does not accept images".into() },
                });
            }
        }
    }
    selected
}

/// Attachment descriptions survive even when the endpoint cannot view their images.
pub(super) fn attachments(body: &EventBody) -> Option<String> {
    let EventBody::UserMessage(message) = body else {
        return None;
    };
    let lines = descriptions(&message.images, &message.unviewable);
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(super) fn descriptions(images: &[ImageRef], unviewable: &[Unviewable]) -> Vec<String> {
    images
        .iter()
        .map(|image| {
            format!(
                "[image: {}, {} bytes; file: {}]",
                image.format.mime(),
                image.size,
                image.path
            )
        })
        .chain(unviewable.iter().map(self::unviewable))
        .collect()
}

pub(super) fn unviewable(attachment: &Unviewable) -> String {
    let reason = match &attachment.reason {
        UnviewableReason::Unsupported => "Enco cannot read this kind of content",
        UnviewableReason::TooLarge => "too large to view as an image",
        UnviewableReason::Unavailable(message) => message,
    };
    format!("[not shown: {}; {reason}]", attachment.what)
}

/// A recorded message's text bytes and envelope, plus a fixed cost for each image it sends.
pub(super) fn message_tokens(message: &Message, include: bool) -> u32 {
    let mut bytes = MESSAGE_OVERHEAD_BYTES;
    let mut images = 0u32;
    for part in &message.parts {
        bytes += match part {
            Part::Text(text) => text.len(),
            Part::ToolCall(call) => call.id.len() + call.name.len() + call.arguments.len(),
            Part::ToolResult(result) => result.call_id.len() + result.content.len(),
            Part::Extension(size) => usize::try_from(*size).unwrap_or(usize::MAX),
            Part::Image(_) => {
                if include {
                    images = images.saturating_add(IMAGE_TOKENS);
                }
                continue;
            }
        }
        .saturating_add(PART_OVERHEAD_BYTES);
    }
    tokens(bytes).saturating_add(images)
}
