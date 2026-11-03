//! Accepts mapper outputs once and converts complete channel records in both directions.
use crate::{
    bindings::{channels as wire, enco::plugin::types},
    convert,
};
use enco_core::{
    DateTime, Failure, FileData, MappingOrigin, Outgoing, QuoteKind, Unviewable, UnviewableReason,
    Utc,
};
use enco_kernel::{
    ChannelAction, ChannelActivityEpilogue, ChannelActivityUpdate, ChannelAttachment,
    ChannelCommand, ChannelCommandKind, ChannelContent, ChannelConversationName, ChannelFault,
    ChannelIdentity, ChannelInput, ChannelMessage, ChannelPacingGap, ChannelQuote, ChannelQuoted,
    ChannelQuotedMessage,
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
        realm,
        conversation_name,
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
    if realm.as_ref().is_some_and(String::is_empty) {
        return Err(convert::bad("channel message realm is empty"));
    }
    let conversation_name = conversation_name
        .map(|name| match name {
            wire::ConversationName::Name(name) => {
                if name.is_empty() {
                    return Err(convert::bad("channel conversation name is empty"));
                }
                Ok(ChannelConversationName::Name(name))
            }
            wire::ConversationName::Fetch(locator) => {
                if locator.is_empty() {
                    return Err(convert::bad("channel conversation name locator is empty"));
                }
                Ok(ChannelConversationName::Fetch(locator))
            }
        })
        .transpose()?;
    let action = match action {
        wire::Action::Message(content) => ChannelAction::Message(self::content(content)?),
        wire::Action::Sessions => ChannelAction::Sessions,
        wire::Action::SelectSession(name) => ChannelAction::SelectSession(name),
        wire::Action::Cancel => ChannelAction::Cancel,
        wire::Action::Backtrack => ChannelAction::Backtrack,
        wire::Action::Everyone(value) => ChannelAction::Everyone(value),
        wire::Action::Halt(value) => ChannelAction::Halt(value),
        wire::Action::Archive(value) => ChannelAction::Archive(value),
    };
    Ok(ChannelMessage {
        key,
        origin,
        input: ChannelInput {
            conversation,
            realm,
            conversation_name,
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

fn content(value: wire::Content) -> Result<ChannelContent<ChannelAttachment>, Failure> {
    let wire::Content {
        text,
        images,
        unviewable,
        quote,
    } = value;
    Ok(ChannelContent {
        text,
        images: images.into_iter().map(attachment).collect(),
        unviewable: unviewable.into_iter().map(self::unviewable).collect(),
        quote: quote.map(self::quote).transpose()?,
    })
}

fn attachment(value: wire::Attachment) -> ChannelAttachment {
    let wire::Attachment { what, locator } = value;
    ChannelAttachment { what, locator }
}

fn unviewable(value: types::Unviewable) -> Unviewable {
    let types::Unviewable { what, reason } = value;
    let reason = match reason {
        types::UnviewableReason::Unsupported => UnviewableReason::Unsupported,
        types::UnviewableReason::TooLarge => UnviewableReason::TooLarge,
        types::UnviewableReason::Unavailable(reason) => UnviewableReason::Unavailable(reason),
    };
    Unviewable { what, reason }
}

fn quote(value: wire::Quote) -> Result<ChannelQuote, Failure> {
    let wire::Quote { kind, messages } = value;
    Ok(ChannelQuote {
        kind: match kind {
            types::QuoteKind::Reply => QuoteKind::Reply,
            types::QuoteKind::Forward => QuoteKind::Forward,
        },
        messages: match messages {
            wire::Quoted::Messages(messages) => ChannelQuoted::Messages(quoted_messages(messages)?),
            wire::Quoted::Fetch(locator) => ChannelQuoted::Fetch(locator),
            wire::Quoted::Unavailable(reason) => ChannelQuoted::Unavailable(reason),
        },
    })
}

/// At least one message, the first at depth 0, each nested at most one level below the one before.
pub(super) fn quoted_messages(
    values: Vec<wire::QuotedMessage>,
) -> Result<Vec<ChannelQuotedMessage>, Failure> {
    if values.is_empty() {
        return Err(convert::bad("quoted messages are empty"));
    }
    let mut deepest = 0;
    for value in &values {
        if value.depth > deepest {
            return Err(convert::bad(
                "a quoted message is nested more than one level below the message before it",
            ));
        }
        deepest = value.depth.saturating_add(1);
    }
    values
        .into_iter()
        .map(|value| {
            let wire::QuotedMessage {
                depth,
                sender,
                sender_name,
                conversation_name,
                sent_at,
                text,
                images,
                unviewable,
            } = value;
            Ok(ChannelQuotedMessage {
                depth,
                sender,
                sender_name,
                conversation_name,
                sent_at: sent_at.map(datetime).transpose()?,
                text,
                images: images.into_iter().map(attachment).collect(),
                unviewable: unviewable.into_iter().map(self::unviewable).collect(),
            })
        })
        .collect()
}

fn datetime(value: types::Datetime) -> Result<DateTime<Utc>, Failure> {
    let types::Datetime {
        seconds,
        nanoseconds,
    } = value;
    let seconds = i64::try_from(seconds)
        .map_err(|_| convert::bad("quoted message time is outside the supported range"))?;
    DateTime::from_timestamp(seconds, nanoseconds)
        .ok_or_else(|| convert::bad("quoted message time is outside the supported range"))
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
            ChannelCommandKind::Halt => wire::CommandKind::Halt,
            ChannelCommandKind::Archive => wire::CommandKind::Archive,
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
                |enco_kernel::ChannelToolActivity {
                     name,
                     state,
                     arguments,
                 }| wire::ToolActivity {
                    name: name.clone(),
                    arguments: arguments.clone(),
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

pub(super) fn activity_update(value: &ChannelActivityUpdate) -> wire::ActivityUpdate {
    match value {
        ChannelActivityUpdate::View(view) => wire::ActivityUpdate::View(activity(view)),
        ChannelActivityUpdate::Epilogue(ChannelActivityEpilogue {
            elapsed_seconds,
            rounds,
            end,
            unsent,
            excerpt,
        }) => wire::ActivityUpdate::Epilogue(wire::ActivityEpilogue {
            elapsed_seconds: *elapsed_seconds,
            rounds: *rounds,
            end: convert::run_end(end),
            unsent: *unsent,
            excerpt: excerpt.clone(),
        }),
        ChannelActivityUpdate::Remove => wire::ActivityUpdate::Remove,
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
    fn quoted_messages_start_at_the_top_and_nest_one_level_at_a_time() {
        let message = |depth| wire::QuotedMessage {
            depth,
            sender: None,
            sender_name: None,
            conversation_name: None,
            sent_at: None,
            text: "text".into(),
            images: vec![],
            unviewable: vec![],
        };
        assert!(quoted_messages(vec![message(0), message(1), message(1), message(0)]).is_ok());
        assert!(quoted_messages(vec![]).is_err());
        assert!(quoted_messages(vec![message(1)]).is_err());
        assert!(quoted_messages(vec![message(0), message(2)]).is_err());
    }

    #[test]
    fn optional_conversation_metadata_must_be_nonempty_when_present() {
        let inbound = |realm: Option<&str>, conversation_name| wire::Inbound {
            key: "event-1".into(),
            conversation: "conversation-1".into(),
            realm: realm.map(str::to_owned),
            conversation_name,
            message: "message-1".into(),
            sender: None,
            sender_name: None,
            direct: true,
            addressed: true,
            reply_to: None,
            respond: None,
            action: wire::Action::Sessions,
        };
        let origin = MappingOrigin {
            call: enco_core::ContentHash::of(b"received"),
            index: 0,
        };
        for (realm, name) in [
            (None, None),
            (
                Some("realm-1"),
                Some(wire::ConversationName::Name("Topic".into())),
            ),
            (
                None,
                Some(wire::ConversationName::Fetch("channel-1".into())),
            ),
        ] {
            assert!(message(inbound(realm, name), origin).is_ok());
        }
        for (realm, name) in [
            (Some(""), None),
            (None, Some(wire::ConversationName::Name(String::new()))),
            (None, Some(wire::ConversationName::Fetch(String::new()))),
        ] {
            let Err(failure) = message(inbound(realm, name), origin) else {
                panic!("empty metadata was accepted");
            };
            assert_eq!(failure.code, enco_core::code::PLUGIN_CONTRACT);
        }
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
