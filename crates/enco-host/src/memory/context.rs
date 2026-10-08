use super::{Memories, Relevance};
use crate::limits::MEMORY_RECALL_DEFAULT;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use std::{collections::HashSet, sync::Arc};
pub struct MemoryContextSource {
    memories: Arc<Memories>,
    relevance: Option<Relevance>,
}

impl MemoryContextSource {
    pub fn new(memories: Arc<Memories>, relevance: Option<Relevance>) -> Self {
        Self {
            memories,
            relevance,
        }
    }
}

#[async_trait]
impl ContextSource for MemoryContextSource {
    async fn contribute(&self, query: &ContextQuery) -> Result<Contribution, ContextError> {
        let mut rows = self
            .memories
            .pinned()
            .await
            .map_err(|e| ContextError(e.to_string()))?;
        let mut omitted = vec![];
        if query.new_input
            && let Some(event) = &query.latest_event
        {
            let text = event.canonical_message().joined_text();
            let recall = self
                .memories
                .recall(&text, MEMORY_RECALL_DEFAULT as usize, &query.cancel)
                .await
                .map_err(|e| ContextError(e.to_string()))?;
            let recalled = recall
                .memories
                .into_iter()
                .chain(recall.unindexed)
                .filter(|memory| !memory.pinned)
                .collect();
            if let Some(relevance) = &self.relevance {
                let filtered = relevance
                    .filter(&text, recalled, &query.cancel)
                    .await
                    .map_err(|e| ContextError(e.to_string()))?;
                rows.extend(filtered.kept);
                omitted.extend(filtered.omitted);
            } else {
                rows.extend(recalled);
            }
            if let Some(failure) = recall.lexical_only {
                omitted.push(Omission {
                    source: "memory:semantic".into(),
                    reason: format!(
                        "embedding failed ({}): {}; memories were recalled by keywords only",
                        failure.code, failure.message
                    ),
                });
            }
        }
        let mut seen = HashSet::new();
        let candidates = rows
            .into_iter()
            .filter(|row| seen.insert(row.id))
            .map(|row| Candidate {
                id: format!("memory:{}", row.id),
                kind: CandidateKind::Memory,
                text: row.text,
                standing: row.pinned,
            })
            .collect();
        Ok(Contribution {
            candidates,
            omitted,
        })
    }
}
