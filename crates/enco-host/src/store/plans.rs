//! Shared tool catalogs and bounded prefix chains encode complete, content-addressed request plans.
//!
//! A plan whose items extend the Session's latest plan stores only what it adds, chained to that
//! plan; any other plan, or one whose chain has reached the limit, is stored in full. Each distinct
//! tool catalog is stored once, and restoring checks the complete content against its address.
use super::{SqliteStore, backend, rows};
use crate::limits::PLAN_CHAIN_LIMIT;
use enco_core::{ContentHash, ContextPlan, Omission, PlanItem, SessionId};
use enco_kernel::{PlanWrite, StoreError};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

struct Record {
    parent: Option<ContentHash>,
    catalog: ContentHash,
    body: Vec<u8>,
}

pub(super) struct EncodedPlan {
    hash: ContentHash,
    records: Vec<Record>,
    catalog: Vec<u8>,
}

enum Preparation {
    Existing,
    New(Option<EncodedPlan>),
}

#[derive(Serialize, Deserialize)]
struct Payload<I, O> {
    items: I,
    max_output_tokens: Option<u32>,
    omitted: O,
}

pub(super) struct EncodedWrite {
    hash: ContentHash,
    catalog_hash: ContentHash,
    catalog: Vec<u8>,
    parent: Option<ContentHash>,
    body: Vec<u8>,
}

impl SqliteStore {
    pub(super) async fn prepare_plan(
        &self,
        session: SessionId,
        write: Arc<PlanWrite>,
    ) -> Result<Option<EncodedWrite>, StoreError> {
        let hash = write.hash();
        let prepared = async {
            let input = self
                .run(move |connection| preparation(connection, session, hash))
                .await?;
            match input {
                Preparation::Existing => Ok(None),
                Preparation::New(previous) => {
                    let candidate = write.clone();
                    tokio::task::spawn_blocking(move || encode(&candidate, previous).map(Some))
                        .await
                        .map_err(backend)?
                }
            }
        }
        .await;
        match prepared {
            // An unreadable predecessor prevents sharing, not recording the complete new plan.
            Err(StoreError::Plan { .. }) => {
                tokio::task::spawn_blocking(move || encode(&write, None).map(Some))
                    .await
                    .map_err(backend)?
            }
            result => result,
        }
    }
}

fn preparation(
    connection: &Connection,
    session: SessionId,
    hash: ContentHash,
) -> Result<Preparation, StoreError> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM context_plans WHERE hash=?)",
            [hash.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if exists {
        return Ok(Preparation::Existing);
    }
    let previous = connection
        .query_row(
            "SELECT plan FROM log WHERE session_id=? AND plan IS NOT NULL
         ORDER BY epoch DESC,seq DESC LIMIT 1",
            [session.to_string()],
            |row| rows::text(row, 0),
        )
        .optional()
        .map_err(backend)?;
    Ok(Preparation::New(
        previous.map(|hash| read(connection, hash)).transpose()?,
    ))
}

pub(super) fn read(connection: &Connection, hash: ContentHash) -> Result<EncodedPlan, StoreError> {
    let mut records = Vec::new();
    let mut next = Some(hash);
    while let Some(address) = next {
        if records.len() == PLAN_CHAIN_LIMIT {
            return Err(failure(hash, "parent chain exceeds the restoration limit"));
        }
        let record = connection
            .query_row(
                "SELECT parent,catalog,body FROM context_plans WHERE hash=?",
                [address.to_string()],
                |row| {
                    Ok(Record {
                        parent: crate::sqlite::optional_text(row, 0)?,
                        catalog: rows::text(row, 1)?,
                        body: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(backend)?
            .ok_or_else(|| failure(hash, format!("missing plan record {address}")))?;
        next = record.parent;
        records.push(record);
    }
    let catalog_hash = records[0].catalog;
    let catalog = connection
        .query_row(
            "SELECT body FROM tool_catalogs WHERE hash=?",
            [catalog_hash.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .ok_or_else(|| failure(hash, format!("missing tool catalog {catalog_hash}")))?;
    Ok(EncodedPlan {
        hash,
        records,
        catalog,
    })
}

pub(super) fn restore(encoded: EncodedPlan) -> Result<ContextPlan, StoreError> {
    let hash = encoded.hash;
    let tools = decompress(&encoded.catalog)
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(backend))
        .map_err(|error| failure(hash, format!("tool catalog: {error}")))?;
    let mut plan = ContextPlan {
        items: Vec::new(),
        tools,
        max_output_tokens: None,
        omitted: Vec::new(),
    };
    for record in encoded.records.into_iter().rev() {
        let payload: Payload<Vec<PlanItem>, Vec<Omission>> = decompress(&record.body)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(backend))
            .map_err(|error| failure(hash, format!("plan record: {error}")))?;
        plan.items.extend(payload.items);
        plan.max_output_tokens = payload.max_output_tokens;
        plan.omitted = payload.omitted;
    }
    if plan.hash().map_err(backend)? != hash {
        return Err(failure(hash, "restored content does not match its address"));
    }
    Ok(plan)
}

fn encode(write: &PlanWrite, previous: Option<EncodedPlan>) -> Result<EncodedWrite, StoreError> {
    let plan = write.plan();
    let mut parent = None;
    let mut items = plan.items.as_slice();
    if let Some(previous) = previous
        && previous.records.len() < PLAN_CHAIN_LIMIT
    {
        let hash = previous.hash;
        let previous = restore(previous)?;
        if items.starts_with(&previous.items) {
            parent = Some(hash);
            items = &items[previous.items.len()..];
        }
    }
    let catalog = serde_json::to_vec(&plan.tools).map_err(backend)?;
    Ok(EncodedWrite {
        hash: write.hash(),
        catalog_hash: ContentHash::of(&catalog),
        catalog: compress(&catalog)?,
        parent,
        body: compress(
            &serde_json::to_vec(&Payload {
                items,
                max_output_tokens: plan.max_output_tokens,
                omitted: &plan.omitted,
            })
            .map_err(backend)?,
        )?,
    })
}

impl EncodedWrite {
    pub(super) fn insert(self, tx: &Transaction<'_>) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO tool_catalogs(hash,body) VALUES (?,?) ON CONFLICT(hash) DO NOTHING",
            params![self.catalog_hash.to_string(), self.catalog],
        )
        .map_err(backend)?;
        tx.execute(
            "INSERT INTO context_plans(hash,catalog,parent,body) VALUES (?,?,?,?)
             ON CONFLICT(hash) DO NOTHING",
            params![
                self.hash.to_string(),
                self.catalog_hash.to_string(),
                self.parent.map(|hash| hash.to_string()),
                self.body
            ],
        )
        .map_err(backend)?;
        Ok(())
    }
}

fn compress(bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    zstd::stream::encode_all(bytes, 3).map_err(backend)
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    zstd::stream::decode_all(bytes).map_err(backend)
}

fn failure(hash: ContentHash, reason: impl Into<String>) -> StoreError {
    StoreError::Plan {
        hash,
        reason: reason.into(),
    }
}
