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
async fn fallback_recomposes_the_plan_for_the_new_endpoints_modalities() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        Err(Failure {
            code: code::PROVIDER_SERVER.into(),
            message: "service unavailable".into(),
            retryable: false,
        }),
        reply("text reply"),
    ]);
    let mut document = configuration();
    document["endpoint"]["backup"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["backup"]["model"] = "text-model".into();
    document["endpoint"]["reply"]["accepts"] = json!(["image"]);
    document["profile"]["default"]["reply"] = json!(["reply", "backup"]);
    let (kernel, _) =
        kernel_configured(root.path(), provider.clone(), document, vec![], |_| {}).await;
    let image = kernel.store_image(PNG).await.unwrap();
    send(&kernel, vec![image]).await;
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(pictures(&requests[0]).len(), 1);
    assert!(pictures(&requests[1]).is_empty());
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

struct ForceImages;

#[async_trait::async_trait]
impl Composer for ForceImages {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        let mut plan = reply_plan(input);
        for item in &mut plan.items {
            if let CompositionItem::Log { images, .. } = item {
                *images = true;
            }
        }
        Ok(Composition::Plan(plan))
    }
}

#[tokio::test]
async fn a_plan_cannot_send_images_to_a_text_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, _) = kernel_configured_composer(
        root.path(),
        provider.clone(),
        configuration(),
        vec![],
        std::sync::Arc::new(ForceImages),
        |_| {},
    )
    .await;
    let image = kernel.store_image(PNG).await.unwrap();
    let (_, log) = send(&kernel, vec![image]).await;
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(matches!(&log.last().unwrap().body, EntryBody::RunEnded {
        end: RunEnd::Failed { failure }, ..
    } if failure.code == code::PLUGIN_CONTRACT));
    kernel.shutdown().await.unwrap();
}
