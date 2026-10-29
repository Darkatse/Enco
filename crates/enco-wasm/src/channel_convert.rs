//! Accepts mapper outputs once and converts complete channel records in both directions.
use crate::{
    bindings::{channels as wire, enco::plugin::types},
    convert,
};
use enco_core::{Failure, FileData, MappingOrigin, Outgoing, Unviewable, UnviewableReason};
use enco_kernel::{
    ChannelAction, ChannelAttachment, ChannelCommand, ChannelCommandKind, ChannelContent,
    ChannelFault, ChannelIdentity, ChannelInput, ChannelMessage, ChannelPacingGap,
};
use std::time::Duration;

/// The declared gap; each part above [`CHANNEL_PACING_MAX`](crate::limits::CHANNEL_PACING_MAX)
/// fails loading.
pub(super) fn pacing(value: wire::PacingGap) -> Result<ChannelPacingGap, String> {
    let wire::PacingGap {
        minimum_ms,
        jitter_ms,
    } = value;
    let max = crate::limits::CHANNEL_PACING_MAX;
    if [minimum_ms, jitter_ms]
        .into_iter()
        .any(|part| Duration::from_millis(part.into()) > max)
    {
        return Err(format!(
            "channel pacing gap exceeds {} s for its minimum or jitter",
            max.as_secs()
        ));
    }
    Ok(ChannelPacingGap {
        minimum_ms,
        jitter_ms,
    })
}

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

pub(super) fn json(value: &str, label: &str) -> Result<serde_json::Value, Failure> {
    serde_json::from_str(value).map_err(|error| convert::bad(format!("{label}: {error}")))
}

pub(super) fn message(
    value: wire::Inbound,
    origin: MappingOrigin,
) -> Result<ChannelMessage, Failure> {
    let wire::Inbound {
        key,
        conversation,
        message,
        sender,
        sender_name,
        direct,
        addressed,
        reply_to,
        respond,
        action,
    } = value;
    if key.is_empty() {
        return Err(convert::bad("channel message key is empty"));
    }
    let action = match action {
        wire::Action::Message(content) => ChannelAction::Message(self::content(content)),
        wire::Action::Sessions => ChannelAction::Sessions,
        wire::Action::SelectSession(name) => ChannelAction::SelectSession(name),
        wire::Action::Cancel => ChannelAction::Cancel,
        wire::Action::Backtrack => ChannelAction::Backtrack,
        wire::Action::Everyone(value) => ChannelAction::Everyone(value),
    };
    Ok(ChannelMessage {
        key,
        origin,
        input: ChannelInput {
            conversation,
            message,
            sender,
            sender_name,
            direct,
            addressed,
            reply_to,
            respond,
            action,
        },
    })
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

pub(super) fn parts(values: Vec<wire::Outgoing>) -> Result<Vec<Outgoing<FileData>>, Failure> {
    let parts: Vec<_> = values.into_iter().map(part).collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.text.is_empty() && part.file.is_none())
    {
        return Err(convert::bad(
            "channel split requires at least one part and no empty parts",
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

pub(super) fn activity(value: &enco_kernel::ChannelActivity) -> wire::ActivityView {
    let enco_kernel::ChannelActivity {
        elapsed_seconds,
        tools,
    } = value;
    wire::ActivityView {
        elapsed_seconds: *elapsed_seconds,
        tools: tools
            .iter()
            .map(
                |enco_kernel::ChannelToolActivity { name, state }| wire::ToolActivity {
                    name: name.clone(),
                    state: match state {
                        enco_kernel::ChannelToolState::Running => wire::ToolState::Running,
                        enco_kernel::ChannelToolState::Ok => wire::ToolState::Ok,
                        enco_kernel::ChannelToolState::Failed => wire::ToolState::Failed,
                        enco_kernel::ChannelToolState::Unknown => wire::ToolState::Unknown,
                    },
                },
            )
            .collect(),
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
    fn split_requires_one_part_and_no_empty_part() {
        let part = |text: &str, file: bool| wire::Outgoing {
            text: text.into(),
            file: file.then(|| wire::FileData {
                name: "a.txt".into(),
                data: b"data".to_vec(),
            }),
        };
        assert!(parts(vec![part("", true)]).is_ok());
        assert!(parts(vec![]).is_err());
        assert!(parts(vec![part("text", false), part("", false)]).is_err());
    }

    #[test]
    fn a_pacing_gap_beyond_the_limit_fails_loading() {
        let limit = u32::try_from(crate::limits::CHANNEL_PACING_MAX.as_millis()).unwrap();
        let gap = |minimum_ms, jitter_ms| wire::PacingGap {
            minimum_ms,
            jitter_ms,
        };
        assert!(pacing(gap(limit, limit)).is_ok());
        assert!(pacing(gap(limit + 1, 0)).is_err());
        assert!(pacing(gap(0, limit + 1)).is_err());
    }
}
