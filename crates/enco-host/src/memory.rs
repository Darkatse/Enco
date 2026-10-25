//! The owner's memories: a SQLite database holds them, and a derived vector index finds them.
//!
//! The index never owns memory content, so deleting it loses nothing; it is rebuilt from the
//! database.
mod authority;
mod context;
mod index;
mod relevance;
mod settings;
mod tools;

use crate::limits::EMBED_BATCH;
use authority::Authority;
pub use context::MemoryContextSource;
use enco_core::*;
use enco_kernel::{Clock, Export, Registry, RegistryError, Verdict, View};
use index::MemoryIndex;
pub use settings::TABLE_OWNER;
use settings::{EmbeddingEndpoint, SettingsCache};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
pub use tools::{memory_tool_specs, memory_tools};

/// Where the memory database and its vector index live; `Memories` owns both.
pub struct MemoryPaths {
    /// The memory database, which holds every memory.
    pub db: PathBuf,
    /// The vector index, derived from the database.
    pub index: PathBuf,
}

/// The memories recalled for a query: those the index found, and recent ones it does not cover yet.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Recall {
    /// The memories the index found, as the database holds them now.
    pub memories: Vec<Memory>,
    /// Recent memories the index does not cover yet, offered without ranking.
    pub unindexed: Vec<Memory>,
    /// Present when embedding failed; callers must disclose keyword-only recall.
    pub lexical_only: Option<Failure>,
}

/// Every memory, and which of them the index does not cover yet.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MemoryList {
    /// Every memory, as the database holds it.
    pub memories: Vec<Memory>,
    /// The memories the index does not cover yet.
    pub unindexed: Vec<MemoryId>,
}

/// Why a memory operation failed.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// The registry failed while the embedding plugin's result was reported to it.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// The memory database failed; the text is its message.
    #[error("memory database: {0}")]
    Authority(String),
    /// Another version of Enco wrote the memory database.
    #[error(
        "memory database schema version {found} does not match this Enco ({expected}); {}",
        if .found > .expected { "upgrade Enco" } else { "migrate the database by hand or recreate it" }
    )]
    SchemaVersion {
        /// The version the database records.
        found: u32,
        /// The version this Enco reads and writes.
        expected: u32,
    },
    /// The vector index cannot be read or written; deleting it while Enco is stopped rebuilds it.
    #[error("memory index: {0}; stop Enco and delete memory-index to rebuild it")]
    Index(String),
    /// The caller cancelled the operation.
    #[error("cancelled")]
    Cancelled,
}

/// Sole entry point for memory writes and recall. The index never owns memory content.
pub struct Memories {
    authority: Authority,
    index: Arc<Mutex<Option<MemoryIndex>>>,
    index_path: PathBuf,
    settings: SettingsCache,
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
}

type IndexGuard = OwnedMutexGuard<Option<MemoryIndex>>;

struct Synced {
    query_vector: Option<Vec<f32>>,
    unindexed: Vec<MemoryId>,
    failure: Option<Failure>,
}

impl Memories {
    /// Open the database and the index at `paths`, then index the memories the index lacks, as far
    /// as the embedding service allows.
    pub async fn open(
        paths: MemoryPaths,
        registry: Arc<Registry>,
        clock: Arc<dyn Clock>,
    ) -> Result<Arc<Self>, MemoryError> {
        let authority = Authority::open(paths.db).await?;
        let memories = Arc::new(Self {
            authority,
            index: Arc::new(Mutex::new(None)),
            index_path: paths.index,
            settings: SettingsCache::default(),
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

    /// Store a new memory and return it with its ID.
    pub async fn save(&self, text: String, pinned: bool) -> Result<Memory, MemoryError> {
        self.authority.save(text, pinned, self.clock.now()).await
    }

    /// Replace a memory's text, its pin, or both; `None` when no memory has this ID.
    pub async fn update(
        &self,
        id: MemoryId,
        text: Option<String>,
        pinned: Option<bool>,
    ) -> Result<Option<Memory>, MemoryError> {
        self.authority
            .update(id, text, pinned, self.clock.now())
            .await
    }

    /// Delete a memory for good; `false` when no memory has this ID.
    pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError> {
        self.authority.forget(id).await
    }

    /// Every pinned memory, oldest first.
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
        let (index_guard, unindexed) = index_work(index_guard, move |index| {
            Ok(unindexed(index.as_ref(), &active))
        })
        .await?;
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
        let (index_guard, hits) = index_work(index_guard, move |index| match &mut **index {
            Some(index) => index.search(&text, vector.as_deref(), limit),
            None => Ok(vec![]),
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
        index_guard: IndexGuard,
        query: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(IndexGuard, Synced), MemoryError> {
        let view = self.registry.view();
        let settings = match self.settings.get(&view) {
            Ok(settings) => settings,
            Err(error) => {
                let active = self.authority.revisions().await?;
                let unindexed = unindexed(index_guard.as_ref(), &active);
                return Ok((
                    index_guard,
                    Synced {
                        query_vector: None,
                        unindexed,
                        failure: Some(config_failure(error)),
                    },
                ));
            }
        };
        let endpoint = &settings.embedding;
        let rows = if index_guard
            .as_ref()
            .is_none_or(|index| index.identity != endpoint.identity())
        {
            self.authority.all(false).await?
        } else {
            vec![]
        };
        let path = self.index_path.clone();
        let requested = endpoint.clone();
        let (index_guard, ()) = index_work(index_guard, move |slot| {
            if slot
                .as_ref()
                .is_none_or(|index| index.identity != requested.identity())
            {
                // Release Trivium's file lock before opening the next vector space.
                **slot = None;
                **slot = Some(MemoryIndex::open(&path, &requested, &rows)?);
            }
            Ok(())
        })
        .await?;
        self.sync_index(index_guard, &view, endpoint, query, cancel)
            .await
    }

    async fn sync_index(
        &self,
        index_guard: IndexGuard,
        view: &View,
        endpoint: &EmbeddingEndpoint,
        query: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(IndexGuard, Synced), MemoryError> {
        let active = self.authority.revisions().await?;
        check_cancel(cancel)?;
        let (index_guard, unindexed) =
            index_write(index_guard, move |index| index.reconcile(&active)).await?;
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

        let result = reported_call(
            &self.registry,
            view.exports.embedding(&endpoint.endpoint),
            cancel,
            |adapter| async move { adapter.embed(&endpoint.endpoint.settings, inputs).await },
        )
        .await?;
        let vectors = match result
            .and_then(|vectors| validate_dimensions(vectors, endpoint.dimensions.get()))
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
            index_write(index_guard, move |index| index.apply(replacements)).await?;
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

/// Invoke one resolved export until cancelled and report its verdict before returning the result.
///
/// An export that cannot be resolved is an ordinary failure with no generation to report.
async fn reported_call<T: ?Sized, R, F>(
    registry: &Registry,
    export: Result<Export<T>, Failure>,
    cancel: &CancellationToken,
    call: impl FnOnce(Arc<T>) -> F,
) -> Result<Result<R, Failure>, MemoryError>
where
    F: Future<Output = Result<R, Failure>>,
{
    let result = match export {
        Ok(Export {
            generation,
            adapter,
            ..
        }) => {
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(Failure {
                    code: code::CANCELLED.into(), message: "plugin call cancelled".into(), retryable: false,
                }),
                result = call(adapter) => result,
            };
            let verdict = match &result {
                Ok(_) => Verdict::Ok,
                Err(failure) => Verdict::Failed(failure.clone()),
            };
            registry.report(generation, verdict, None).await?;
            result
        }
        Err(failure) => Err(failure),
    };
    check_cancel(cancel)?;
    Ok(result)
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

/// A failed write may leave the node map behind the file, so the index closes and the next use
/// reopens it; cancellation and service failures keep it open.
async fn index_write<T: Send + 'static>(
    index_guard: IndexGuard,
    work: impl FnOnce(&mut MemoryIndex) -> Result<T, MemoryError> + Send + 'static,
) -> Result<(IndexGuard, T), MemoryError> {
    index_work(index_guard, move |slot| {
        let index = slot
            .as_mut()
            .ok_or_else(|| index_error("index is not open"))?;
        work(index).inspect_err(|_| **slot = None)
    })
    .await
}

// Move the owned guard into blocking IO and return it to keep sync + search serialized.
// Callers await completion even when cancelled, so no index write outlives its owner.
async fn index_work<S: Send + 'static, T: Send + 'static>(
    mut state: S,
    work: impl FnOnce(&mut S) -> Result<T, MemoryError> + Send + 'static,
) -> Result<(S, T), MemoryError> {
    tokio::task::spawn_blocking(move || {
        let value = work(&mut state)?;
        Ok((state, value))
    })
    .await
    .map_err(index_error)?
}

fn validate_dimensions(
    vectors: Vec<Vec<f32>>,
    dimensions: usize,
) -> Result<Vec<Vec<f32>>, Failure> {
    let valid = vectors.iter().all(|vector| vector.len() == dimensions);
    if !valid {
        return Err(Failure {
            code: code::PROVIDER_BAD_RESPONSE.into(),
            message: "embedding dimensions do not match the index configuration".into(),
            retryable: false,
        });
    }
    Ok(vectors)
}

fn unindexed(index: Option<&MemoryIndex>, active: &[authority::Revision]) -> Vec<MemoryId> {
    index.map_or_else(
        || active.iter().map(|row| row.id).collect(),
        |index| index.unindexed(active),
    )
}

fn config_failure(error: enco_kernel::SettingsError) -> Failure {
    Failure {
        code: code::CONFIG_INVALID.into(),
        message: error.to_string(),
        retryable: false,
    }
}
