//! Checks channel records accepted at the mapper boundary.
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
        attachments: vec![],
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
