mod authority;
mod context;
mod index;
mod tools;

use crate::limits::EMBED_BATCH;
use authority::Authority;
pub use context::MemoryContextSource;
use enco_core::*;
use enco_kernel::{Clock, Registry};
use index::MemoryIndex;
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
pub use tools::memory_tools;

/// Authority and derived-index locations, both owned by Memories.
pub struct MemoryPaths {
    pub db: PathBuf,
    pub index: PathBuf,
}

/// Configured embedding caller. The adapter is resolved for each synchronization call.
#[derive(Clone)]
pub struct EmbeddingEndpoint {
    /// Registered plugin whose embedding interface is used.
    pub plugin: String,
    /// Durable service parameters, excluding the credential itself.
    pub settings: ProviderSettings,
    /// Credential held only in memory.
    pub api_key: Option<String>,
    /// Expected vector width, paired with the model name to identify the derived index.
    pub dimensions: usize,
}

/// Current authority records nominated by the index, plus recent unindexed records.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Recall {
    pub memories: Vec<Memory>,
    pub unindexed: Vec<Memory>,
    /// Present when embedding failed; callers must disclose keyword-only recall.
    pub lexical_only: Option<Failure>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MemoryList {
    pub memories: Vec<Memory>,
    pub unindexed: Vec<MemoryId>,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory database: {0}")]
    Authority(String),
    #[error("memory database was created by a newer Enco (schema version {0})")]
    NewerSchema(u32),
    #[error("memory index: {0}; stop Enco and delete memory-index to rebuild it")]
    Index(String),
    #[error("cancelled")]
    Cancelled,
}

/// Sole entry point for memory writes and recall. The index never owns memory content.
pub struct Memories {
    authority: Authority,
    index: Arc<Mutex<MemoryIndex>>,
    embedding: EmbeddingEndpoint,
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
}

struct Synced {
    query_vector: Option<Vec<f32>>,
    unindexed: Vec<MemoryId>,
    failure: Option<Failure>,
}

impl Memories {
    pub async fn open(
        paths: MemoryPaths,
        embedding: EmbeddingEndpoint,
        registry: Arc<Registry>,
        clock: Arc<dyn Clock>,
    ) -> Result<Arc<Self>, MemoryError> {
        let authority = Authority::open(paths.db).await?;
        let active = authority.all(false).await?;
        let model = embedding.settings.model.clone();
        let dimensions = embedding.dimensions;
        let index = tokio::task::spawn_blocking(move || {
            MemoryIndex::open(&paths.index, &model, dimensions, &active)
        })
        .await
        .map_err(index_error)??;
        let memories = Arc::new(Self {
            authority,
            index: Arc::new(Mutex::new(index)),
            embedding,
            registry,
            clock,
        });

        // Startup drains missing batches; runtime recall advances one batch per request.
        let cancel = CancellationToken::new();
        let mut previous_unindexed = usize::MAX;
        loop {
            let index_guard = memories.index.clone().lock_owned().await;
            let (index_guard, synced) = memories.sync(index_guard, None, &cancel).await?;
            drop(index_guard);
            if synced.unindexed.is_empty()
                || synced.failure.is_some()
                || synced.unindexed.len() >= previous_unindexed
            {
                break;
            }
            previous_unindexed = synced.unindexed.len();
        }
        Ok(memories)
    }

    pub async fn save(&self, text: String, pinned: bool) -> Result<Memory, MemoryError> {
        self.authority
            .save(text, pinned, self.clock.now().to_utc())
            .await
    }

    pub async fn update(
        &self,
        id: MemoryId,
        text: Option<String>,
        pinned: Option<bool>,
    ) -> Result<Option<Memory>, MemoryError> {
        self.authority
            .update(id, text, pinned, self.clock.now().to_utc())
            .await
    }

    pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError> {
        self.authority.forget(id).await
    }

    pub async fn pinned(&self) -> Result<Vec<Memory>, MemoryError> {
        let mut rows = self.authority.all(true).await?;
        rows.sort_by_key(|m| m.id);
        Ok(rows)
    }

    /// Inspect the authority and index revisions without contacting the embedding service.
    pub async fn list(&self) -> Result<MemoryList, MemoryError> {
        let index_guard = self.index.clone().lock_owned().await;
        let memories = self.authority.all(false).await?;
        let active = memories
            .iter()
            .map(|m| authority::Revision {
                id: m.id,
                rev: m.rev,
            })
            .collect::<Vec<_>>();
        let (index_guard, unindexed) =
            index_work(index_guard, move |index| Ok(index.unindexed(&active))).await?;
        drop(index_guard);
        Ok(MemoryList {
            memories,
            unindexed,
        })
    }

    /// Synchronize and search under one index lock, then resolve IDs against the authority.
    pub async fn recall(
        &self,
        query: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Recall, MemoryError> {
        let index_guard = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(MemoryError::Cancelled),
            index_guard = self.index.clone().lock_owned() => index_guard,
        };
        let (index_guard, synced) = self.sync(index_guard, Some(query), cancel).await?;
        check_cancel(cancel)?;

        let text = query.to_owned();
        let vector = synced.query_vector;
        let (index_guard, hits) = index_work(index_guard, move |index| {
            index.search(&text, vector.as_deref(), limit)
        })
        .await?;
        drop(index_guard);
        check_cancel(cancel)?;

        // The index nominates IDs; only the authority supplies returned content.
        let memories = self.authority.get(hits).await?;
        let recalled_ids: HashSet<_> = memories.iter().map(|m| m.id).collect();
        let ids = synced
            .unindexed
            .into_iter()
            .filter(|id| !recalled_ids.contains(id))
            .take(limit)
            .collect();
        let unindexed = self.authority.get(ids).await?;
        check_cancel(cancel)?;
        Ok(Recall {
            memories,
            unindexed,
            lexical_only: synced.failure,
        })
    }

    async fn sync(
        &self,
        index_guard: OwnedMutexGuard<MemoryIndex>,
        query: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(OwnedMutexGuard<MemoryIndex>, Synced), MemoryError> {
        // Authority supplies newest-first revisions; reconciliation preserves that order.
        let active = self.authority.revisions().await?;
        check_cancel(cancel)?;
        let (index_guard, unindexed) =
            index_work(index_guard, move |index| index.reconcile(&active)).await?;
        check_cancel(cancel)?;

        // Reserve one embedding slot for the query; startup uses the same batch path.
        let batch = unindexed.iter().take(EMBED_BATCH - 1).copied().collect();
        let rows = self.authority.get(batch).await?;
        check_cancel(cancel)?;

        let mut inputs: Vec<String> = query.map(str::to_owned).into_iter().collect();
        inputs.extend(rows.iter().map(|m| m.text.clone()));
        if inputs.is_empty() {
            return Ok((
                index_guard,
                Synced {
                    query_vector: None,
                    unindexed: vec![],
                    failure: None,
                },
            ));
        }

        let count = inputs.len();
        let result = match self.registry.exports().embedding(&self.embedding.plugin) {
            Ok(export) => tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(MemoryError::Cancelled),
                result = export.adapter.embed(&self.embedding.settings, self.embedding.api_key.as_deref(), inputs) => result,
            },
            Err(failure) => Err(failure),
        };
        check_cancel(cancel)?;
        let vectors = match result
            .and_then(|vectors| validate_embeddings(vectors, count, self.embedding.dimensions))
        {
            Ok(vectors) => vectors,
            Err(failure) => {
                return Ok((
                    index_guard,
                    Synced {
                        query_vector: None,
                        unindexed,
                        failure: Some(failure),
                    },
                ));
            }
        };

        let mut vectors = vectors.into_iter();
        let query_vector = if query.is_some() {
            vectors.next()
        } else {
            None
        };
        let inserted: HashSet<_> = rows.iter().map(|m| m.id).collect();
        let replacements = rows.into_iter().zip(vectors).collect();
        let (index_guard, ()) =
            index_work(index_guard, move |index| index.apply(replacements)).await?;
        check_cancel(cancel)?;

        Ok((
            index_guard,
            Synced {
                query_vector,
                unindexed: unindexed
                    .into_iter()
                    .filter(|id| !inserted.contains(id))
                    .collect(),
                failure: None,
            },
        ))
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), MemoryError> {
    if cancel.is_cancelled() {
        Err(MemoryError::Cancelled)
    } else {
        Ok(())
    }
}

fn index_error(e: impl std::fmt::Display) -> MemoryError {
    MemoryError::Index(e.to_string())
}

// Move the owned guard into blocking IO and return it to keep sync + search serialized.
// Callers await completion even when cancelled, so no index write outlives its owner.
async fn index_work<T: Send + 'static>(
    mut index_guard: OwnedMutexGuard<MemoryIndex>,
    work: impl FnOnce(&mut MemoryIndex) -> Result<T, MemoryError> + Send + 'static,
) -> Result<(OwnedMutexGuard<MemoryIndex>, T), MemoryError> {
    tokio::task::spawn_blocking(move || {
        let value = work(&mut index_guard)?;
        Ok((index_guard, value))
    })
    .await
    .map_err(index_error)?
}

fn validate_embeddings(
    vectors: Vec<Vec<f32>>,
    count: usize,
    dimensions: usize,
) -> Result<Vec<Vec<f32>>, Failure> {
    let valid = vectors.len() == count
        && vectors.iter().all(|vector| {
            vector.len() == dimensions && vector.iter().all(|value| value.is_finite())
        });
    if !valid {
        return Err(Failure {
            code: code::PROVIDER_BAD_RESPONSE.into(),
            message: "embedding count, dimensions or numeric values do not match the request"
                .into(),
            retryable: false,
        });
    }
    Ok(vectors)
}
