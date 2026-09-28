use super::{EmbeddingSpec, MemoryError, authority::Revision};
use enco_core::{Memory, MemoryId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use triviumdb::{Database, SearchConfig, TriviumError, database::Config};

pub(super) struct MemoryIndex {
    db: Database<f32>,
    nodes: HashMap<MemoryId, IndexedMemory>,
}

#[derive(Clone, Copy)]
struct IndexedMemory {
    node: u64,
    rev: u64,
}

struct Reconciled {
    delete: Vec<u64>,
    keep: HashMap<MemoryId, IndexedMemory>,
    unindexed: Vec<MemoryId>,
}

#[derive(Serialize, Deserialize, PartialEq)]
struct Identity {
    model: String,
    dimensions: usize,
}

#[derive(Deserialize)]
struct Payload {
    memory: MemoryId,
    rev: u64,
}

fn error(e: impl std::fmt::Display) -> MemoryError {
    MemoryError::Index(e.to_string())
}

fn reconcile(
    active: &[Revision],
    nodes: impl IntoIterator<Item = (u64, Option<Payload>)>,
) -> Reconciled {
    let active_by_id: HashMap<_, _> = active.iter().map(|m| (m.id, m.rev)).collect();
    let nodes: Vec<_> = nodes.into_iter().collect();
    let mut counts = HashMap::new();
    for (_, value) in &nodes {
        if let Some(payload) = value {
            *counts.entry(payload.memory).or_insert(0) += 1;
        }
    }
    let mut keep = HashMap::new();
    let mut delete = vec![];
    for (node, value) in nodes {
        if let Some(payload) = value
            && active_by_id.get(&payload.memory) == Some(&payload.rev)
            && counts.get(&payload.memory) == Some(&1)
        {
            keep.insert(
                payload.memory,
                IndexedMemory {
                    node,
                    rev: payload.rev,
                },
            );
        } else {
            delete.push(node);
        }
    }
    let unindexed = active
        .iter()
        .filter(|row| !keep.contains_key(&row.id))
        .map(|row| row.id)
        .collect();
    Reconciled {
        delete,
        keep,
        unindexed,
    }
}

impl MemoryIndex {
    pub fn open(
        path: &Path,
        embedding: &EmbeddingSpec,
        active: &[Memory],
    ) -> Result<Self, MemoryError> {
        let identity = Identity {
            model: embedding.model.clone(),
            dimensions: embedding.dimensions,
        };
        let previous = std::fs::read(path.join("index.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Identity>(&bytes).ok());
        if previous.as_ref() != Some(&identity) {
            reset(path, &identity)?;
        }
        let file = path.join("memory.tdb");
        let file = file
            .to_str()
            .ok_or_else(|| error("index path is not UTF-8"))?;
        let config = Config {
            dim: embedding.dimensions,
            load_text_index: false,
            ..Default::default()
        };
        let db = match Database::<f32>::open_with_config(file, config) {
            Ok(db) => db,
            Err(e @ TriviumError::DatabaseLocked(_)) => return Err(error(e)),
            Err(_) => {
                reset(path, &identity)?;
                Database::open_with_config(file, config).map_err(error)?
            }
        };
        let source = db.get_all_ids().into_iter().map(|node| {
            (
                node,
                db.get_payload(node)
                    .and_then(|payload| serde_json::from_value::<Payload>(payload).ok()),
            )
        });
        let revisions: Vec<_> = active
            .iter()
            .map(|m| Revision {
                id: m.id,
                rev: m.rev,
            })
            .collect();
        let reconciled = reconcile(&revisions, source);
        let mut index = Self {
            db,
            nodes: reconciled.keep,
        };
        index.remove(&reconciled.delete)?;
        // Trivium persists vectors in its WAL, but its text index must be rebuilt.
        for row in active {
            if let Some(indexed) = index.nodes.get(&row.id) {
                index
                    .db
                    .index_text(indexed.node, &row.text)
                    .map_err(error)?;
            }
        }
        index.db.build_text_index().map_err(error)?;
        index.db.flush().map_err(error)?;
        Ok(index)
    }

    pub fn reconcile(&mut self, active: &[Revision]) -> Result<Vec<MemoryId>, MemoryError> {
        let reconciled = self.compare_revisions(active);
        self.remove(&reconciled.delete)?;
        self.nodes = reconciled.keep;
        Ok(reconciled.unindexed)
    }

    fn remove(&mut self, ids: &[u64]) -> Result<(), MemoryError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.db.delete_many_atomic(ids).map_err(error)?;
        let deleted: HashSet<_> = ids.iter().copied().collect();
        self.nodes
            .retain(|_, indexed| !deleted.contains(&indexed.node));
        Ok(())
    }

    pub fn apply(&mut self, rows: Vec<(Memory, Vec<f32>)>) -> Result<(), MemoryError> {
        for (row, vector) in rows {
            if let Some(previous) = self.nodes.get(&row.id).copied() {
                self.remove(&[previous.node])?;
            }
            let node = self
                .db
                .insert(&vector, json!({ "memory": row.id, "rev": row.rev }))
                .map_err(error)?;
            self.nodes
                .insert(row.id, IndexedMemory { node, rev: row.rev });
            self.db.index_text(node, &row.text).map_err(error)?;
        }
        self.db.build_text_index().map_err(error)
    }

    pub fn search(
        &self,
        query: &str,
        vector: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<MemoryId>, MemoryError> {
        let hits = self
            .db
            .search_hybrid(
                Some(query),
                vector,
                &SearchConfig {
                    top_k: limit,
                    expand_depth: 0,
                    min_score: 0.,
                    enable_text_hybrid_search: true,
                    ..Default::default()
                },
            )
            .map_err(error)?;
        let mut seen = HashSet::new();
        Ok(hits
            .into_iter()
            .filter_map(|hit| {
                self.db
                    .get_payload(hit.id)
                    .and_then(|v| serde_json::from_value::<Payload>(v).ok())
                    .map(|p| p.memory)
            })
            .filter(|id| seen.insert(*id))
            .collect())
    }

    pub fn unindexed(&self, active: &[Revision]) -> Vec<MemoryId> {
        self.compare_revisions(active).unindexed
    }

    fn compare_revisions(&self, active: &[Revision]) -> Reconciled {
        reconcile(
            active,
            self.nodes.iter().map(|(id, indexed)| {
                (
                    indexed.node,
                    Some(Payload {
                        memory: *id,
                        rev: indexed.rev,
                    }),
                )
            }),
        )
    }
}

fn reset(path: &Path, identity: &Identity) -> Result<(), MemoryError> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(error)?;
    }
    std::fs::create_dir_all(path).map_err(error)?;
    std::fs::write(
        path.join("index.json"),
        serde_json::to_vec(identity).map_err(error)?,
    )
    .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciliation_rejects_duplicate_invalid_and_stale_nodes() {
        let duplicate = MemoryId::new();
        let stale = MemoryId::new();
        let current = MemoryId::new();
        let active = [
            Revision {
                id: duplicate,
                rev: 2,
            },
            Revision { id: stale, rev: 3 },
            Revision {
                id: current,
                rev: 4,
            },
        ];
        let reconciled = reconcile(
            &active,
            [
                (
                    10,
                    Some(Payload {
                        memory: duplicate,
                        rev: 2,
                    }),
                ),
                (
                    11,
                    Some(Payload {
                        memory: duplicate,
                        rev: 2,
                    }),
                ),
                (12, None),
                (
                    13,
                    Some(Payload {
                        memory: stale,
                        rev: 2,
                    }),
                ),
                (
                    14,
                    Some(Payload {
                        memory: current,
                        rev: 4,
                    }),
                ),
            ],
        );
        assert_eq!(reconciled.delete, [10, 11, 12, 13]);
        assert_eq!(reconciled.unindexed, [duplicate, stale]);
        assert_eq!(reconciled.keep.len(), 1);
        assert_eq!(reconciled.keep[&current].node, 14);
        assert_eq!(reconciled.keep[&current].rev, 4);
    }
}
