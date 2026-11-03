//! Checks recorded input projection and parsing of identities returned in a guest plan.
use super::{composition, wire};
use enco_core::*;
use enco_kernel::TranscriptItem;
use std::{collections::HashMap, path::PathBuf};

fn reply(hash: String, node: String) -> wire::Composition {
    wire::Composition::Reply(wire::Plan {
        items: vec![wire::PlanItem::Message(wire::Inline {
            role: crate::bindings::enco::plugin::types::Role::System,
            text: "instructions".into(),
            sources: vec![wire::Source {
                id: "instructions:AGENTS.md".into(),
                hash,
            }],
        })],
        tools: vec![wire::CapabilityId {
            node,
            name: "fs_read".into(),
        }],
        max_output_tokens: None,
        omitted: vec![],
    })
}

#[test]
fn malformed_guest_identities_are_contract_failures() {
    let bad_hash = composition(reply("invalid".into(), NodeId::new().to_string()));
    assert!(
        matches!(bad_hash, Err(failure) if failure.code == code::PLUGIN_CONTRACT && failure.message.contains("source hash"))
    );
    let bad_node = composition(reply(
        ContentHash::of(b"source").to_string(),
        "invalid".into(),
    ));
    assert!(
        matches!(bad_node, Err(failure) if failure.code == code::PLUGIN_CONTRACT && failure.message.contains("capability node"))
    );
}

fn quoted<A>(attachments: Vec<A>, depth: u8) -> QuotedMessage<A> {
    QuotedMessage {
        depth,
        sender: Some("42".into()),
        sender_name: Some("Ada".into()),
        owner: false,
        yours: false,
        conversation_name: None,
        sent_at: None,
        text: "Look here".into(),
        attachments,
        unviewable: vec![],
    }
}

fn image(size: u8) -> Image {
    Image {
        format: ImageFormat::Png,
        size: u64::from(size),
        content: ContentHash::of(&[size]),
    }
}

fn file(size: u8) -> File {
    File {
        name: format!("report-{size}.pdf"),
        size: u64::from(size),
        content: ContentHash::of(&[size]),
    }
}

fn event(body: EventBody) -> Event {
    Event {
        id: EventId::new(),
        session: SessionId::new(),
        received_at: "2026-10-29T12:00:00Z".parse().unwrap(),
        source: EventSource::Cli,
        body,
    }
}

fn transcript_item(event: Event) -> TranscriptItem {
    TranscriptItem {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(1),
        },
        message: event.canonical_message(),
        generation: None,
        event: Some(event),
    }
}

#[test]
fn recorded_attachments_keep_their_order_and_locations_in_inputs_and_quotes() {
    let event = event(EventBody::UserMessage {
        text: "Describe these".into(),
        attachments: vec![Attachment::Image(image(4)), Attachment::File(file(5))],
        unviewable: vec![],
        earlier: vec![EarlierMessage {
            received_at: "2026-10-29T11:00:00Z".parse().unwrap(),
            sender: None,
            sender_name: None,
            owner: false,
            text: "Earlier".into(),
            attachments: vec![],
            origin: MappingOrigin {
                call: ContentHash::of(b"earlier"),
                index: 0,
            },
            quote: Some(EarlierQuote {
                kind: QuoteKind::Reply,
                messages: Some(Quoted::Messages(vec![quoted(
                    vec!["voice message".into()],
                    0,
                )])),
            }),
        }],
        quote: Some(Quote {
            kind: QuoteKind::Forward,
            messages: Quoted::Messages(vec![
                quoted(
                    vec![Attachment::File(file(1)), Attachment::Image(image(2))],
                    0,
                ),
                quoted(
                    vec![Attachment::Image(image(3)), Attachment::File(file(6))],
                    1,
                ),
            ]),
            fetched: None,
        }),
    });
    let paths = (1..=6)
        .map(|size| {
            (
                ContentHash::of(&[size]),
                PathBuf::from(format!("/blobs/{size}")),
            )
        })
        .collect();
    let item = super::transcript_item(&transcript_item(event), &paths).unwrap();
    let wire::EventBody::UserMessage(message) = item.event.unwrap().body else {
        panic!("user input")
    };
    assert_eq!(
        attachment_paths(&message.attachments),
        ["/blobs/4", "/blobs/5"]
    );
    let wire::AttachmentRef::File(file) = &message.attachments[1] else {
        panic!("file attachment")
    };
    assert_eq!((file.name.as_str(), file.size), ("report-5.pdf", 5));
    assert_eq!(file.content, ContentHash::of(&[5]).to_string());
    let wire::Quoted::Messages(messages) = message.quote.unwrap().messages else {
        panic!("quoted messages")
    };
    assert_eq!(
        attachment_paths(&messages[0].attachments),
        ["/blobs/1", "/blobs/2"]
    );
    assert_eq!(
        attachment_paths(&messages[1].attachments),
        ["/blobs/3", "/blobs/6"]
    );
    let wire::EarlierQuoted::Messages(messages) = message.earlier[0]
        .quote
        .as_ref()
        .unwrap()
        .messages
        .as_ref()
        .unwrap()
    else {
        panic!("earlier quoted messages")
    };
    assert_eq!(messages[0].attachments, ["voice message"]);
}

fn attachment_paths(attachments: &[wire::AttachmentRef]) -> Vec<&str> {
    attachments
        .iter()
        .map(|attachment| match attachment {
            wire::AttachmentRef::Image(image) => image.path.as_str(),
            wire::AttachmentRef::File(file) => file.path.as_str(),
        })
        .collect()
}

#[test]
fn session_files_use_the_same_content_addressed_paths() {
    let item = transcript_item(event(EventBody::SessionMessage(Outgoing {
        text: "Done".into(),
        file: Some(file(7)),
    })));
    let paths = [(ContentHash::of(&[7]), PathBuf::from("/blobs/7"))].into();
    let projected = super::transcript_item(&item, &paths).unwrap();
    let wire::EventBody::SessionMessage(message) = projected.event.unwrap().body else {
        panic!("Session message")
    };
    assert_eq!(message.file.unwrap().path, "/blobs/7");
    assert!(super::transcript_item(&item, &HashMap::new()).is_err());
}
