//! Checks recorded input projection and parsing of identities returned in a guest plan.
use super::{composition, wire};
use enco_core::*;
use enco_kernel::{LocatedImage, TranscriptItem};

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

fn quoted(images: Vec<Image>, depth: u8) -> QuotedMessage<Image> {
    QuotedMessage {
        depth,
        sender: Some("42".into()),
        sender_name: Some("Ada".into()),
        owner: false,
        yours: false,
        conversation_name: None,
        sent_at: None,
        text: "Look here".into(),
        images,
        unviewable: vec![],
    }
}

fn quoted_event() -> Event {
    let at: DateTime<Utc> = "2026-10-29T12:00:00Z".parse().unwrap();
    let image = |size| Image {
        format: ImageFormat::Png,
        size,
        content: ContentHash::of(&[u8::try_from(size).unwrap()]),
    };
    let origin = MappingOrigin {
        call: ContentHash::of(b"mapping"),
        index: 0,
    };
    Event {
        id: EventId::new(),
        session: SessionId::new(),
        received_at: at,
        source: EventSource::Channel {
            protocol: "test".into(),
            account: "9".into(),
            conversation: "chat".into(),
            sender: "42".into(),
            sender_name: Some("Ada".into()),
            owner: true,
            origin,
            message: "message-1".into(),
        },
        body: EventBody::UserMessage {
            text: "Describe these".into(),
            images: vec![image(4)],
            unviewable: vec![],
            earlier: vec![],
            quote: Some(Quote {
                kind: QuoteKind::Forward,
                messages: Quoted::Messages(vec![
                    quoted(vec![image(1)], 0),
                    quoted(vec![image(2), image(3)], 1),
                ]),
                fetched: Some(origin),
            }),
        },
    }
}

#[test]
fn recorded_quote_images_keep_their_locations_and_do_not_become_own_images() {
    let event = quoted_event();
    let canonical = event.canonical_message();
    let message = Message {
        role: canonical.role,
        parts: canonical
            .parts
            .into_iter()
            .map(|part| {
                part.retype().unwrap_or_else(|image: Image| {
                    Part::Image(LocatedImage {
                        path: format!("/images/{}.png", image.size).into(),
                        image,
                    })
                })
            })
            .collect(),
    };
    let item = super::transcript_item(&TranscriptItem {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(1),
        },
        message,
        generation: None,
        event: Some(event),
    })
    .unwrap();
    let wire::EventBody::UserMessage(message) = item.event.unwrap().body else {
        panic!("user input")
    };
    assert_eq!(
        message
            .images
            .iter()
            .map(|image| image.path.as_str())
            .collect::<Vec<_>>(),
        ["/images/4.png"]
    );
    let wire::Quoted::Messages(messages) = message.quote.unwrap().messages else {
        panic!("quoted messages")
    };
    assert_eq!(
        messages[0]
            .images
            .iter()
            .map(|image| image.path.as_str())
            .collect::<Vec<_>>(),
        ["/images/1.png"]
    );
    assert_eq!(
        messages[1]
            .images
            .iter()
            .map(|image| image.path.as_str())
            .collect::<Vec<_>>(),
        ["/images/2.png", "/images/3.png"]
    );
}
