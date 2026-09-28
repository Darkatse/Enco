#![allow(clippy::unwrap_used, clippy::expect_used)]
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

async fn open(root: &Path, provider: Arc<dyn Provider>, dimensions: usize) -> Arc<Memories> {
    Memories::open(
        MemoryPaths {
            db: root.join("memory.db"),
            index: root.join("memory-index"),
        },
        EmbeddingSpec {
            model: "test-embedding".into(),
            dimensions,
        },
        provider,
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
    kernel_with(root, provider, |deps, _| {
        deps.context
            .push(Arc::new(MemoryContextSource::new(memories.clone())));
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
    let provider = ScriptedProvider::new(vec![
        calls(vec![call(
            "memory_save",
            r#"{"text":"Owner likes coffee","pinned":true}"#,
        )]),
        reply("saved"),
    ]);
    let memories = open(dir.path(), provider.clone(), 64).await;
    let kernel = memory_kernel(dir.path(), provider.clone(), memories.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    send(&kernel, session.id, "remember my drink").await;
    let saved = memories.list().await.unwrap().memories.pop().unwrap();
    assert!(
        provider.requests.lock().unwrap()[1].messages[0]
            .joined_text()
            .contains("Owner likes coffee")
    );
    provider.steps.lock().unwrap().extend([
        calls(vec![call(
            "memory_update",
            &serde_json::json!({ "id": saved.id, "text": "Owner likes tea" }).to_string(),
        )]),
        reply("corrected"),
    ]);
    send(&kernel, session.id, "correct my drink").await;
    let system = provider.requests.lock().unwrap()[3].messages[0].joined_text();
    assert!(system.contains("Owner likes tea"));
    assert!(!system.contains("Owner likes coffee"));
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
        provider.requests.lock().unwrap().last().unwrap().messages[0]
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
        !provider.requests.lock().unwrap().last().unwrap().messages[0]
            .joined_text()
            .contains("Owner likes tea")
    );
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
    let pinned = memories
        .save("主人的名字是 River".into(), true)
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
    let source = MemoryContextSource::new(memories.clone());
    let session = SessionRecord {
        id: SessionId::new(),
        name: "context".into(),
        created_at: Utc::now(),
        binding: Binding {
            node: NodeId::new(),
            epoch: Epoch(1),
        },
        config: SessionConfig::default(),
    };
    let query = ContextQuery {
        latest_event: Some(Event {
            id: EventId::new(),
            session: session.id,
            source: EventSource::Cli,
            body: EventBody::UserMessage {
                text: "completely unrelated".into(),
            },
            received_at: Utc::now(),
        }),
        session,
        cancel: cancel.clone(),
    };
    assert!(
        source
            .contribute(&query)
            .await
            .unwrap()
            .candidates
            .iter()
            .any(|c| c.id == format!("memory:{}", pinned.id))
    );
    provider.embedding_failure.store(true, Ordering::SeqCst);
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
    assert!(
        source
            .contribute(&query)
            .await
            .unwrap()
            .omitted
            .iter()
            .any(|o| o.source == "memory:semantic")
    );
    provider.embedding_failure.store(false, Ordering::SeqCst);
    memories.recall("陶艺", 20, &cancel).await.unwrap();
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(source);
    drop(memories);
    std::fs::remove_dir_all(dir.path().join("memory-index")).unwrap();
    let memories = open(dir.path(), provider.clone(), 64).await;
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(memories);
    provider.embedding_dimensions.store(32, Ordering::SeqCst);
    let memories = open(dir.path(), provider.clone(), 32).await;
    assert!(memories.list().await.unwrap().unindexed.is_empty());
    drop(memories);
    provider.embedding_failure.store(true, Ordering::SeqCst);
    std::fs::remove_dir_all(dir.path().join("memory-index")).unwrap();
    let memories = open(dir.path(), provider, 32).await;
    assert_eq!(memories.list().await.unwrap().unindexed.len(), 21);
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
impl Provider for GatedEmbedding {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "embedding-gate".into(),
            version: "test".into(),
        }
    }

    async fn complete(&self, _: ProviderRequest) -> Result<Completion, Failure> {
        reply("unused")
    }

    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
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
