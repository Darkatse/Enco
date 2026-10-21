use super::encoded_tokens;
use crate::limits::{IMAGE_TOKENS, REQUEST_IMAGE_MAX_BYTES};
use enco_core::*;
use enco_kernel::*;
use std::collections::BTreeSet;

pub(super) fn selected(input: &ComposeInput, omitted: &mut Vec<Omission>) -> BTreeSet<LogPos> {
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
            selected.insert(item.pos);
        } else {
            remaining = 0;
            for image in images {
                omitted.push(Omission {
                    source: format!("image:{}:{}:{}", item.pos.epoch.0, item.pos.seq.0, image.content),
                    reason: if accepts {
                        format!("entry's images do not fit the {REQUEST_IMAGE_MAX_BYTES}-byte image budget")
                    } else { "reply endpoint does not accept images".into() },
                });
            }
        }
    }
    selected
}

/// One line per attachment the owner sent, whether or not this request shows it.
pub(super) fn attachments(input: &ComposeInput, body: &EventBody) -> Option<String> {
    let EventBody::UserMessage {
        images, unviewable, ..
    } = body
    else {
        return None;
    };
    let images = images.iter().map(|image| {
        format!(
            "[image: {}, {} bytes; file: {}]",
            image.format.mime(),
            image.size,
            input.image_paths[&image.content].display()
        )
    });
    let unviewable = unviewable.iter().map(|attachment| {
        let reason = match &attachment.reason {
            UnviewableReason::Unsupported => "unsupported attachment format".into(),
            UnviewableReason::TooLarge => format!("exceeds the {IMAGE_MAX_BYTES}-byte image limit"),
            UnviewableReason::Unavailable(message) => message.clone(),
        };
        format!("[attachment unavailable: {}: {reason}]", attachment.what)
    });
    let lines: Vec<_> = images.chain(unviewable).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(super) fn message_tokens(message: &Message, include: bool) -> Result<u32, ComposeError> {
    let mut text = Message {
        role: message.role,
        parts: Vec::new(),
    };
    let mut tokens: u32 = 0;
    for part in &message.parts {
        match part {
            Part::Image(_) if include => tokens = tokens.saturating_add(IMAGE_TOKENS),
            Part::Image(_) => {}
            part => text.parts.push(part.clone()),
        }
    }
    Ok(tokens.saturating_add(encoded_tokens(&text)?))
}
