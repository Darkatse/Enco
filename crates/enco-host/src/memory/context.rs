use super::Memories;
use crate::limits::MEMORY_RECALL_DEFAULT;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use std::{collections::HashSet, sync::Arc};
pub struct MemoryContextSource {
    memories: Arc<Memories>,
}

impl MemoryContextSource {
    pub fn new(memories: Arc<Memories>) -> Self {
        Self { memories }
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
        let text = query
            .latest_event
            .as_ref()
            .filter(|_| query.new_input)
            .map(|event| event.canonical_message().joined_text())
            .filter(|text| !text.is_empty());
        if let Some(text) = text {
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
            let filtered = self
                .memories
                .filter_relevance(&text, recalled, &query.cancel)
                .await
                .map_err(|e| ContextError(e.to_string()))?;
            rows.extend(filtered.kept);
            omitted.extend(filtered.omitted);
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
