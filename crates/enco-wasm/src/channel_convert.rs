//! Accepts mapper outputs once and converts complete channel records in both directions.
use crate::{
    bindings::{channels as wire, enco::plugin::types},
    convert,
};
use enco_core::{Failure, FileData, MappingOrigin, Outgoing, Unviewable, UnviewableReason};
use enco_kernel::{
    ChannelAction, ChannelAttachment, ChannelCommand, ChannelCommandKind, ChannelContent,
    ChannelFault, ChannelIdentity, ChannelInput, ChannelUpdate,
};
use std::time::Duration;

pub(super) fn identity(value: &ChannelIdentity) -> wire::Identity {
    let ChannelIdentity {
        protocol,
        account,
        facts,
    } = value;
    wire::Identity {
        protocol: protocol.clone(),
        account: account.clone(),
        facts: facts.to_string(),
    }
}

pub(super) fn identified(value: wire::Identity) -> Result<ChannelIdentity, Failure> {
    let wire::Identity {
        protocol,
        account,
        facts,
    } = value;
    if protocol.is_empty() || account.is_empty() || protocol.contains(':') {
        return Err(convert::bad(
            "channel protocol and account must be nonempty; the protocol name must contain no colon",
        ));
    }
    Ok(ChannelIdentity {
        protocol,
        account,
        facts: json(&facts, "channel identity facts")?,
    })
}

fn json(value: &str, label: &str) -> Result<serde_json::Value, Failure> {
    serde_json::from_str(value).map_err(|error| convert::bad(format!("{label}: {error}")))
}

pub(super) fn update(value: wire::Update, origin: MappingOrigin) -> Result<ChannelUpdate, Failure> {
    let wire::Update {
        key,
        state,
        message,
    } = value;
    if key.is_empty() {
        return Err(convert::bad("channel update key is empty"));
    }
    Ok(ChannelUpdate {
        key,
        protocol: json(&state, "channel update state")?,
        input: message.map(input),
        origin,
    })
}

fn input(value: wire::Inbound) -> ChannelInput {
    let wire::Inbound {
        conversation,
        message,
        sender,
        sender_name,
        direct,
        addressed,
        reply_to,
        action,
    } = value;
    let action = match action {
        wire::Action::Message(content) => ChannelAction::Message(self::content(content)),
        wire::Action::Sessions => ChannelAction::Sessions,
        wire::Action::SelectSession(name) => ChannelAction::SelectSession(name),
        wire::Action::Cancel => ChannelAction::Cancel,
        wire::Action::Backtrack => ChannelAction::Backtrack,
        wire::Action::Everyone(value) => ChannelAction::Everyone(value),
    };
    ChannelInput {
        conversation,
        message,
        sender,
        sender_name,
        direct,
        addressed,
        reply_to,
        action,
    }
}

fn content(value: wire::Content) -> ChannelContent<ChannelAttachment> {
    let wire::Content {
        text,
        images,
        unviewable,
    } = value;
    ChannelContent {
        text,
        images: images
            .into_iter()
            .map(|wire::Attachment { what, locator }| ChannelAttachment { what, locator })
            .collect(),
        unviewable: unviewable
            .into_iter()
            .map(|types::Unviewable { what, reason }| {
                let reason = match reason {
                    types::UnviewableReason::Unsupported => UnviewableReason::Unsupported,
                    types::UnviewableReason::TooLarge => UnviewableReason::TooLarge,
                    types::UnviewableReason::Unavailable(reason) => {
                        UnviewableReason::Unavailable(reason)
                    }
                };
                Unviewable { what, reason }
            })
            .collect(),
    }
}

pub(super) fn outgoing(value: &Outgoing<FileData>) -> wire::Outgoing {
    let Outgoing { text, file } = value;
    wire::Outgoing {
        text: text.clone(),
        file: file.as_ref().map(|FileData { name, data }| wire::FileData {
            name: name.clone(),
            data: data.clone(),
        }),
    }
}

fn part(value: wire::Outgoing) -> Outgoing<FileData> {
    let wire::Outgoing { text, file } = value;
    Outgoing {
        text,
        file: file.map(|wire::FileData { name, data }| FileData { name, data }),
    }
}

pub(super) fn parts(
    values: Vec<wire::Outgoing>,
    message: &Outgoing<FileData>,
) -> Result<Vec<Outgoing<FileData>>, Failure> {
    let parts: Vec<_> = values.into_iter().map(part).collect();
    if parts
        .iter()
        .any(|part| part.text.is_empty() && part.file.is_none())
        || !parts
            .iter()
            .flat_map(|part| part.text.bytes())
            .eq(message.text.bytes())
        || !parts
            .iter()
            .filter_map(|part| part.file.as_ref())
            .eq(message.file.iter())
    {
        return Err(convert::bad(
            "channel split must preserve all text and exactly the original file, without empty parts",
        ));
    }
    Ok(parts)
}

pub(super) fn command(value: &ChannelCommand) -> wire::Command {
    let ChannelCommand {
        kind,
        description,
        direct,
        group,
    } = value;
    wire::Command {
        kind: match kind {
            ChannelCommandKind::Session => wire::CommandKind::Session,
            ChannelCommandKind::Cancel => wire::CommandKind::Cancel,
            ChannelCommandKind::Backtrack => wire::CommandKind::Backtrack,
            ChannelCommandKind::Everyone => wire::CommandKind::Everyone,
        },
        description: description.clone(),
        direct: *direct,
        group: *group,
    }
}

pub(super) fn fault(value: wire::Fault, origin: Option<MappingOrigin>) -> ChannelFault {
    let wire::Fault {
        failure,
        unknown,
        fatal,
        retry_after,
    } = value;
    ChannelFault {
        failure: convert::failure(failure),
        unknown,
        fatal,
        retry_after: retry_after.map(|seconds| Duration::from_secs(seconds.into())),
        origin,
    }
}

pub(super) fn failure(
    failure: Failure,
    unknown: bool,
    origin: Option<MappingOrigin>,
) -> ChannelFault {
    ChannelFault {
        failure,
        unknown,
        fatal: true,
        retry_after: None,
        origin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_parts_reject_lost_or_duplicated_content_at_the_guest_boundary() {
        let message = Outgoing {
            text: "hello世界".into(),
            file: Some(FileData {
                name: "a.txt".into(),
                data: b"data".to_vec(),
            }),
        };
        let first = wire::Outgoing {
            text: "hello".into(),
            file: None,
        };
        let last = wire::Outgoing {
            text: "世界".into(),
            file: Some(wire::FileData {
                name: "a.txt".into(),
                data: b"data".to_vec(),
            }),
        };
        assert_eq!(
            parts(vec![first.clone(), last.clone()], &message)
                .unwrap()
                .len(),
            2
        );
        assert!(parts(vec![first.clone()], &message).is_err());
        assert!(parts(vec![first, last.clone(), last], &message).is_err());
    }
}
