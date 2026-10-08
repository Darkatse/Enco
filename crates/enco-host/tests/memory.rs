#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

async fn open(root: &Path, provider: Arc<dyn Embedding>, dimensions: usize) -> Arc<Memories> {
    let (registry, _) = registry(
        &root.join("embedding-registry"),
        Loaded {
            lifecycle: Arc::new(Probe(Ok(()))),
            summary: "embedding".into(),
            completion: None,
            embedding: Some(provider),
            decision: None,
        },
    )
    .await;
    Memories::open(
        MemoryPaths {
            db: root.join("memory.db"),
            index: root.join("memory-index"),
        },
        embedding_endpoint("test-embedding", dimensions),
        registry,
        Arc::new(SystemClock),
    )
    .await
    .unwrap()
}

async fn memory_kernel(
    root: &Path,
    provider: Arc<ScriptedProvider>,
    memories: Arc<Memories>,
) -> Kernel {
    kernel_with(root, provider, |deps| {
        deps.context
            .push(Arc::new(MemoryContextSource::new(memories.clone(), None)));
        deps.tools.extend(memory_tools(memories));
    })
    .await
    .0
}

async fn send(kernel: &Kernel, session: SessionId, text: &str) {
    let mut rx = kernel.subscribe(session).unwrap();
    kernel
        .submit(session, EventId::new(), text.into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
}

#[tokio::test]
async fn memory_tools_correction_forgetting_and_restart_share_the_authority() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let memories = open(dir.path(), provider.clone(), 64).await;
    let saved = memories
        .save("Owner likes coffee".into(), true)
        .await
        .unwrap();
    let kernel = memory_kernel(dir.path(), provider.clone(), memories.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    provider.steps.lock().unwrap().extend([
        calls(vec![call(
            "memory_update",
            &serde_json::json!({ "id": saved.id, "text": "Owner likes tea" }).to_string(),
        )]),
        reply("corrected"),
    ]);
    send(&kernel, session.id, "correct my drink").await;
    let context = provider.requests.lock().unwrap()[1]
        .messages
        .first()
        .unwrap()
        .joined_text();
    assert!(context.contains("Owner likes tea"));
    assert!(!context.contains("Owner likes coffee"));
    kernel.shutdown().await.unwrap();
    drop(kernel);
    drop(memories);
    let memories = open(dir.path(), provider.clone(), 64).await;
    provider
        .steps
        .lock()
        .unwrap()
        .push_back(reply("after restart"));
    let kernel = memory_kernel(dir.path(), provider.clone(), memories.clone()).await;
    send(&kernel, session.id, "unrelated topic").await;
    assert!(
        provider
            .requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .messages
            .first()
            .unwrap()
            .joined_text()
            .contains("Owner likes tea")
    );
    provider.steps.lock().unwrap().extend([
        calls(vec![call(
            "memory_forget",
            &serde_json::json!({ "id": saved.id }).to_string(),
        )]),
        reply("forgotten"),
    ]);
    send(&kernel, session.id, "forget it").await;
    assert!(memories.list().await.unwrap().memories.is_empty());
    assert!(
        !provider
            .requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .messages
            .first()
            .unwrap()
            .joined_text()
            .contains("Owner likes tea")
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn recalled_sources_keep_corrections_and_deduplicate_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new((0..4).map(|_| reply("recalled")).collect());
    let memories = open(dir.path(), provider.clone(), 64).await;
    let first = "Owner likes tea";
    let second = "Owner likes coffee";
    let memory = memories.save(first.into(), false).await.unwrap();
    provider.steps.lock().unwrap().push_front(calls(vec![call(
        "memory_update",
        &serde_json::json!({ "id": memory.id, "text": second }).to_string(),
    )]));
    let kernel = memory_kernel(dir.path(), provider.clone(), memories.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    send(&kernel, session.id, "What drink do I like?").await;
    send(&kernel, session.id, "What drink do I like now?").await;
    memories
        .update(memory.id, Some(first.into()), None)
        .await
        .unwrap();
    send(&kernel, session.id, "What drink do I like again?").await;
    kernel.shutdown().await.unwrap();
    drop(kernel);
    drop(memories);

    let memories = open(dir.path(), provider.clone(), 64).await;
    let kernel = memory_kernel(dir.path(), provider, memories).await;
    send(&kernel, session.id, "What drink do I like?").await;
    let inspection = kernel.inspect(session.id, None).await.unwrap();
    let source_id = format!("memory:{}", memory.id);
    let mut recalled = Vec::new();
    for item in &inspection.plan.items {
        if let PlanItem::Message { message, sources } = item {
            for source in sources.iter().filter(|source| source.id == source_id) {
                recalled.push((source.hash, message.joined_text()));
            }
        }
    }
    assert_eq!(recalled.len(), 3);
    for ((hash, note), expected) in recalled.iter().zip([first, second, first]) {
        assert_eq!(*hash, ContentHash::of(expected.as_bytes()));
        assert!(note.contains(expected));
    }
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn recall_uses_current_records_during_embedding_failure_and_rebuilds_derived_data() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let memories = open(dir.path(), provider.clone(), 64).await;
    let tea = memories
        .save("主人喜欢喝茉莉花茶".into(), false)
        .await
        .unwrap();
    for i in 0..18 {
        memories
            .save(format!("项目 {i} 的会议安排在星期二"), false)
            .await
            .unwrap();
    }
    let cancel = CancellationToken::new();
    let recall = memories.recall("茉莉花茶", 8, &cancel).await.unwrap();
    assert!(recall.memories.iter().any(|m| m.id == tea.id));
    *provider.embedding_failure.lock().unwrap() = Some(Failure {
        code: code::PROVIDER_NETWORK.into(),
        message: "embedding service unavailable".into(),
        retryable: true,
    });
    memories
        .update(tea.id, Some("主人现在喜欢喝普洱茶".into()), None)
        .await
        .unwrap();
    let fresh = memories
        .save("主人最近在学陶艺".into(), false)
        .await
        .unwrap();
    let recall = memories.recall("茶", 20, &cancel).await.unwrap();
    assert!(recall.lexical_only.is_some());
    assert!(recall.unindexed.iter().any(|m| m.id == fresh.id));
    assert!(
        recall
            .unindexed
            .iter()
            .any(|m| m.id == tea.id && m.text.contains("普洱茶"))
    );
    assert!(
        !recall
            .memories
            .iter()
            .chain(&recall.unindexed)
            .any(|m| m.text.contains("茉莉花茶"))
    );
    *provider.embedding_failure.lock().unwrap() = None;
    memories.recall("陶艺", 20, &cancel).await.unwrap();
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(memories);
    std::fs::remove_dir_all(dir.path().join("memory-index")).unwrap();
    let memories = open(dir.path(), provider.clone(), 64).await;
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(memories);
    provider.embedding_dimensions.store(32, Ordering::SeqCst);
    let memories = open(dir.path(), provider.clone(), 32).await;
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(memories);
    *provider.embedding_failure.lock().unwrap() = Some(Failure {
        code: code::PROVIDER_NETWORK.into(),
        message: "embedding service unavailable".into(),
        retryable: true,
    });
    std::fs::remove_dir_all(dir.path().join("memory-index")).unwrap();
    let memories = open(dir.path(), provider, 32).await;
    assert_eq!(memories.list().await.unwrap().unindexed.len(), 20);
    assert!(
        !memories
            .recall("陶艺", 8, &cancel)
            .await
            .unwrap()
            .unindexed
            .is_empty()
    );
}

struct GatedEmbedding {
    blocked: AtomicBool,
    entered: Notify,
}

#[async_trait]
impl Embedding for GatedEmbedding {
    async fn embed(
        &self,
        _: &ProviderSettings,
        _: Option<&str>,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        if self.blocked.load(Ordering::SeqCst) {
            self.entered.notify_one();
            std::future::pending::<()>().await;
        }
        Ok(vectors(&inputs, 64))
    }
}

#[tokio::test]
async fn cancellation_needs_no_dirty_state_and_writes_do_not_wait_for_embedding() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(GatedEmbedding {
        blocked: AtomicBool::new(false),
        entered: Notify::new(),
    });
    let memories = open(dir.path(), provider.clone(), 64).await;
    let memory = memories.save("old statement".into(), false).await.unwrap();
    provider.blocked.store(true, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let actor = memories.clone();
    let recall = tokio::spawn(async move { actor.recall("statement", 8, &token).await });
    tokio::time::timeout(Duration::from_secs(10), provider.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        memories.update(memory.id, Some("corrected statement".into()), None),
    )
    .await
    .unwrap()
    .unwrap();
    cancel.cancel();
    assert!(matches!(recall.await.unwrap(), Err(MemoryError::Cancelled)));
    provider.blocked.store(false, Ordering::SeqCst);
    let result = memories
        .recall("corrected statement", 8, &CancellationToken::new())
        .await
        .unwrap();
    assert!(
        result
            .memories
            .iter()
            .any(|m| m.text == "corrected statement")
    );
    assert!(memories.list().await.unwrap().unindexed.is_empty());
}
