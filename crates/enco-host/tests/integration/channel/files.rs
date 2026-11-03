//! Protocol attachments are classified only by the kernel, including quoted and member files.
use super::*;
use enco_core::Attachment as StoredAttachment;

fn attachment(name: &str, locator: &str) -> Attachment {
    Attachment {
        name: name.into(),
        locator: locator.into(),
    }
}

#[tokio::test]
async fn files_and_images_keep_protocol_order_and_quoted_documents_are_stored() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("received")]);
    let mut h = harness(dir.path(), provider.clone()).await;
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    let content = MessageContent {
        text: "inspect these".into(),
        attachments: vec![
            attachment("report.pdf", "pdf"),
            attachment("not-a-picture.bin", "png"),
            attachment("large.png", "large"),
            attachment("photo.heic", "heic"),
            attachment("moving.gif", "gif"),
        ],
        unviewable: vec![],
        quote: Some(enco_kernel::ChannelQuote {
            kind: QuoteKind::Reply,
            messages: enco_kernel::ChannelQuoted::Messages(vec![quoted_message(
                "owner",
                "Owner",
                "the original document",
                vec![attachment("quoted.pdf", "quoted")],
            )]),
        }),
    };
    h.updates
        .send(vec![update(1, Action::Message(content))])
        .await
        .unwrap();
    let mut large = PNG.to_vec();
    large.resize(10 * 1024 * 1024 + 1, 0);
    for (locator, bytes) in [
        ("quoted", b"%PDF quoted".to_vec()),
        ("pdf", b"%PDF report".to_vec()),
        ("png", PNG.to_vec()),
        ("large", large),
        ("heic", b"ftypheic".to_vec()),
        ("gif", b"GIF89a".to_vec()),
    ] {
        let read = receive(&mut h.fetching).await;
        assert_eq!(read.locator, locator);
        read.result.send(Ok(fetched(Ok(bytes)))).unwrap();
    }
    receive(&mut h.polling).await;
    receive(&mut h.sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let session = h.kernel.open_session("main").await.unwrap();
    let event = h
        .kernel
        .log(session.id, None)
        .await
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry.body {
            EntryBody::EventConsumed { event } => Some(event),
            _ => None,
        })
        .unwrap();
    let EventBody::UserMessage {
        attachments,
        quote: Some(quote),
        ..
    } = event.body
    else {
        panic!("expected attachments")
    };
    assert_eq!(attachments.len(), 5);
    assert!(matches!(&attachments[0], StoredAttachment::File(file) if file.name == "report.pdf"));
    assert!(
        matches!(&attachments[1], StoredAttachment::Image(image) if image.format == ImageFormat::Png)
    );
    for stored in &attachments[2..] {
        assert!(matches!(stored, StoredAttachment::File(_)));
    }
    let Quoted::Messages(messages) = quote.messages else {
        panic!("expected quoted document")
    };
    let StoredAttachment::File(file) = &messages[0].attachments[0] else {
        panic!("expected file")
    };
    assert_eq!(
        h.store.get_blob(&file.content).await.unwrap(),
        b"%PDF quoted"
    );
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_open_group_accepts_a_members_document() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("received")]);
    let mut document = configuration();
    document["profile"]["group"] = json!({"reply": ["reply"], "compaction": ["compaction"], "tools": [], "owner_context": false, "requires_lifeline": false});
    let (kernel, store) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    let mut h = controlled(kernel, store);
    let mut policy = settings();
    policy.group_profile = Some("group".parse().unwrap());
    let channel = start_with(&h.kernel, &h.adapter, policy);
    receive(&mut h.polling).await;
    let mut open = update(1, Action::Everyone(Some(true)));
    open.input.direct = false;
    h.updates.send(vec![open]).await.unwrap();
    receive(&mut h.polling).await;
    receive(&mut h.sending).await.result.send(Ok(())).unwrap();
    let mut member = update(
        2,
        Action::Message(MessageContent {
            text: String::new(),
            attachments: vec![attachment("member.pdf", "document")],
            unviewable: vec![],
            quote: None,
        }),
    );
    member.input.direct = false;
    member.input.sender = Some("member".into());
    h.updates.send(vec![member]).await.unwrap();
    receive(&mut h.fetching)
        .await
        .result
        .send(Ok(fetched(Ok(b"%PDF member".to_vec()))))
        .unwrap();
    receive(&mut h.polling).await;
    receive(&mut h.sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let session = h.kernel.sessions().await.unwrap().remove(0);
    assert!(h.kernel.log(session.id, None).await.unwrap().iter().any(|entry| matches!(&entry.body,
        EntryBody::EventConsumed { event: Event { source: EventSource::Channel { owner: false, .. }, body: EventBody::UserMessage { attachments, .. }, .. } }
            if matches!(attachments.as_slice(), [StoredAttachment::File(file)] if file.name == "member.pdf"))));
    h.kernel.shutdown().await.unwrap();
}
