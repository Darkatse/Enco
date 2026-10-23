#![expect(
    clippy::unwrap_used,
    reason = "image behavior fixtures fail with context"
)]
use crate::support;

use enco_core::*;
use enco_kernel::*;
use serde_json::json;
use support::*;

const PNG: &[u8] = include_bytes!("../../../enco-kernel/src/image/probe.png");

async fn send(kernel: &Kernel, images: Vec<Image>) -> (SessionId, Vec<Entry>) {
    let session = kernel.open_session("pictures").await.unwrap();
    let mut events = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), String::new(), images)
        .await
        .unwrap();
    (session.id, finish(&mut events).await)
}

fn pictures(request: &ProviderRequest) -> Vec<&ImageData> {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            Part::Image(image) => Some(image),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn owner_and_tool_images_are_recorded_by_reference_and_resolved_for_requests() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("fs_read", r#"{"path":"picture.png"}"#)]),
        reply("seen"),
    ]);
    let mut document = configuration();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    let (kernel, store) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    std::fs::write(root.path().join("workspace/picture.png"), PNG).unwrap();
    let image = kernel.store_image(PNG).await.unwrap();
    let (session, log) = send(&kernel, vec![image.clone()]).await;
    assert!(log.iter().any(|entry| matches!(&entry.body,
        EntryBody::EventConsumed { event: Event { body: EventBody::UserMessage { images, .. }, .. } }
        if images == std::slice::from_ref(&image))));
    assert!(log.iter().any(|entry| matches!(&entry.body,
        EntryBody::ToolCallSettled { outcome: Settlement::Ok, images, .. } if images == std::slice::from_ref(&image))));
    assert_eq!(store.get_blob(&image.content).await.unwrap(), PNG);
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(pictures(&requests[0]).len(), 1);
    assert_eq!(pictures(&requests[1]).len(), 2);
    assert!(
        pictures(&requests[1])
            .iter()
            .all(|sent| sent.data == PNG && sent.format == ImageFormat::Png)
    );
    let inspection = kernel.inspect(session, None).await.unwrap();
    let referenced: Vec<_> = inspection
        .request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            Part::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(referenced, [&image, &image]);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn fallback_recomposes_images_for_the_new_endpoint_and_records_the_omission() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        Err(Failure {
            code: code::PROVIDER_SERVER.into(),
            message: "service unavailable".into(),
            retryable: false,
        }),
        reply("I can inspect the file"),
    ]);
    let mut document = configuration();
    document["endpoint"]["backup"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["backup"]["model"] = "text-model".into();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    document["profile"]["default"]["reply"] = json!(["reply", "backup"]);
    let (kernel, store) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    let image = kernel.store_image(PNG).await.unwrap();
    let (session, _) = send(&kernel, vec![image.clone()]).await;
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(pictures(&requests[0]).len(), 1);
    assert!(pictures(&requests[1]).is_empty());
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| !message.joined_text().is_empty())
    );
    let path = store.blob_path(&image.content).display().to_string();
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.joined_text().contains(&path))
    );
    let inspection = kernel.inspect(session, None).await.unwrap();
    assert!(
        inspection
            .plan
            .omitted
            .iter()
            .any(|omission| omission.source.ends_with(&image.content.to_string()))
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unviewable_attachment_alone_still_reaches_the_model_as_a_description() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("Please send the content as text")]);
    let (kernel, _) = kernel(root.path(), provider.clone()).await;
    let session = kernel.open_session("voice").await.unwrap();
    let mut events = kernel.subscribe(session.id).unwrap();
    kernel
        .accept(
            &[Event {
                id: EventId::new(),
                session: session.id,
                source: EventSource::Channel {
                    channel: "fixture".into(),
                    account: "bot".into(),
                    conversation: "private".into(),
                    sender: "owner".into(),
                },
                received_at: Utc::now(),
                body: EventBody::UserMessage {
                    text: String::new(),
                    images: vec![],
                    unviewable: vec![Unviewable {
                        what: "voice message".into(),
                        reason: UnviewableReason::Unsupported,
                    }],
                },
            }],
            None,
        )
        .await
        .unwrap();
    finish(&mut events).await;
    let requests = provider.requests.lock().unwrap().clone();
    assert!(
        requests[0]
            .messages
            .iter()
            .all(|message| !message.joined_text().is_empty())
    );
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.joined_text().contains("voice message: unsupported"))
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_image_content_fails_before_an_attempt_and_leaves_provider_health_alone() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let mut document = configuration();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    let (kernel, store) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    let before = store.registry().await.unwrap().generations;
    let image = kernel.store_image(PNG).await.unwrap();
    std::fs::remove_file(store.blob_path(&image.content)).unwrap();
    let (_, log) = send(&kernel, vec![image]).await;
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(
        !log.iter()
            .any(|entry| matches!(entry.body, EntryBody::AttemptStarted { .. }))
    );
    assert!(matches!(&log.last().unwrap().body, EntryBody::RunEnded {
        end: RunEnd::Failed { failure }, ..
    } if failure.code == code::CONTEXT_FAILED));
    assert_eq!(store.registry().await.unwrap().generations, before);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn one_message_exceeding_the_image_budget_is_omitted_as_a_whole() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("inspect the files")]);
    let mut document = configuration();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    let (kernel, _) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    let mut bytes = PNG.to_vec();
    bytes.resize(9 * 1024 * 1024, 0);
    let image = kernel.store_image(&bytes).await.unwrap();
    let (session, _) = send(&kernel, vec![image; 4]).await;
    assert!(pictures(&provider.requests.lock().unwrap()[0]).is_empty());
    assert_eq!(
        kernel
            .inspect(session, None)
            .await
            .unwrap()
            .plan
            .omitted
            .len(),
        4
    );
    bytes.resize(usize::try_from(IMAGE_MAX_BYTES).unwrap() + 1, 0);
    assert!(matches!(
        kernel.store_image(&bytes).await,
        Err(ImageError::TooLarge { .. })
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn history_images_stop_at_the_first_entry_that_does_not_fit() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("seen"); 4]);
    let mut document = configuration();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    let (kernel, _) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    let mut bytes = PNG.to_vec();
    bytes.resize(4 * 1024 * 1024, 0);
    let small = kernel.store_image(&bytes).await.unwrap();
    bytes.resize(9 * 1024 * 1024, 0);
    let large = kernel.store_image(&bytes).await.unwrap();
    let session = kernel.open_session("pictures").await.unwrap();
    let mut events = kernel.subscribe(session.id).unwrap();
    let mut positions = Vec::new();
    for images in [
        vec![small],
        vec![large.clone(); 2],
        vec![large.clone(); 2],
        vec![large],
    ] {
        kernel
            .submit(session.id, EventId::new(), "describe".into(), images)
            .await
            .unwrap();
        let log = finish(&mut events).await;
        positions.push(
            log.iter()
                .find(|entry| matches!(entry.body, EntryBody::EventConsumed { .. }))
                .unwrap()
                .pos,
        );
    }
    let inspection = kernel.inspect(session.id, None).await.unwrap();
    let selected: Vec<_> = inspection
        .plan
        .items
        .iter()
        .filter_map(|item| match item {
            PlanItem::Log { pos, images } if positions.contains(pos) => Some(*images),
            _ => None,
        })
        .collect();
    // The newest entries use 27 MiB. The intervening 18 MiB entry stops selection,
    // even though the oldest 4 MiB entry would fit on its own.
    assert_eq!(selected, [false, false, true, true]);
    assert_eq!(
        pictures(provider.requests.lock().unwrap().last().unwrap()).len(),
        3
    );
    assert_eq!(inspection.plan.omitted.len(), 3);
    kernel.shutdown().await.unwrap();
}

struct ForceImages(enco_host::FactoryComposer);

impl Composer for ForceImages {
    fn code(&self) -> CodeRef {
        self.0.code()
    }

    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError> {
        let mut composition = self.0.compose(input)?;
        if let Composition::Plan(plan) = &mut composition {
            for item in &mut plan.items {
                if let PlanItem::Log { images, .. } = item {
                    *images = true;
                }
            }
        }
        Ok(composition)
    }
}

#[tokio::test]
async fn a_plan_cannot_send_images_to_a_text_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, _) = kernel_with(root.path(), provider.clone(), Tz::UTC, |deps| {
        deps.composer = std::sync::Arc::new(ForceImages(enco_host::FactoryComposer::new(
            root.path().join("workspace"),
            root.path().join("AGENTS.md"),
        )));
    })
    .await;
    let image = kernel.store_image(PNG).await.unwrap();
    let (_, log) = send(&kernel, vec![image]).await;
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(matches!(&log.last().unwrap().body, EntryBody::RunEnded {
        end: RunEnd::Failed { failure }, ..
    } if failure.code == code::PLAN_INVALID));
    kernel.shutdown().await.unwrap();
}
