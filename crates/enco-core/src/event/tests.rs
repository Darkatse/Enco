use super::*;
use crate::{ContentHash, File, Image, ImageFormat, Quoted, QuotedMessage};

fn image(bytes: &[u8]) -> Image {
    Image {
        format: ImageFormat::Png,
        size: bytes.len() as u64,
        content: ContentHash::of(bytes),
    }
}

#[test]
fn canonical_input_keeps_quoted_and_own_images_in_order_without_turning_files_into_images() {
    let quoted_image = image(b"quoted");
    let own_image = image(b"own");
    let file = Attachment::File(File {
        name: "report.pdf".into(),
        size: 3,
        content: ContentHash::of(b"pdf"),
    });
    let event = Event {
        id: EventId::new(),
        session: SessionId::new(),
        source: EventSource::Cli,
        body: EventBody::UserMessage {
            text: "read these".into(),
            attachments: vec![file.clone(), Attachment::Image(own_image.clone())],
            unviewable: vec![],
            earlier: vec![],
            quote: Some(Quote {
                kind: crate::QuoteKind::Reply,
                fetched: None,
                messages: Quoted::Messages(vec![QuotedMessage {
                    depth: 0,
                    sender: None,
                    sender_name: None,
                    owner: true,
                    yours: false,
                    conversation_name: None,
                    sent_at: None,
                    text: "quoted text".into(),
                    attachments: vec![Attachment::Image(quoted_image.clone()), file],
                    unviewable: vec![],
                }]),
            }),
        },
        received_at: Utc::now(),
    };
    let recorded: Event = serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
    assert_eq!(recorded, event);
    assert_eq!(
        recorded.canonical_message().parts,
        vec![
            Part::Text {
                text: "read these".into()
            },
            Part::Image(quoted_image),
            Part::Image(own_image),
        ]
    );
}
