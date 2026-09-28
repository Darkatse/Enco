use super::Memories;
use crate::{limits::*, tools::args::*};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use serde_json::{Map, Value, json};
use std::sync::Arc;
enum Kind {
    Save,
    Update,
    Forget,
    Search,
}

struct MemoryTool {
    memories: Arc<Memories>,
    kind: Kind,
}

pub fn memory_tools(memories: Arc<Memories>) -> Vec<Arc<dyn Tool>> {
    [Kind::Save, Kind::Update, Kind::Forget, Kind::Search]
        .into_iter()
        .map(|kind| {
            Arc::new(MemoryTool {
                memories: memories.clone(),
                kind,
            }) as Arc<dyn Tool>
        })
        .collect()
}

#[async_trait]
impl Tool for MemoryTool {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "host-memory".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn spec(&self) -> ToolSpec {
        let (name, description, effect, properties, required) = match self.kind {
            Kind::Save => (
                "memory_save",
                format!(
                    "Save a durable fact about the owner as one self-contained statement. Include dates \
                     when time-bound. Pin only facts useful in almost every conversation. text is at \
                     most {MEMORY_TEXT_BYTES} UTF-8 bytes."
                ),
                Effect::SideEffect,
                json!({
                    "text": { "type": "string" },
                    "pinned": { "type": "boolean" },
                }),
                vec!["text"],
            ),
            Kind::Update => (
                "memory_update",
                format!(
                    "Correct a saved memory by its id. Supply text, pinned, or both; correction replaces \
                     the old statement. text is at most {MEMORY_TEXT_BYTES} UTF-8 bytes."
                ),
                Effect::SideEffect,
                json!({
                    "id": { "type": "string" },
                    "text": { "type": "string" },
                    "pinned": { "type": "boolean" },
                }),
                vec!["id"],
            ),
            Kind::Forget => (
                "memory_forget",
                "Delete a memory from the current memory authority. Historical conversation logs are unchanged."
                    .into(),
                Effect::SideEffect,
                json!({ "id": { "type": "string" } }),
                vec!["id"],
            ),
            Kind::Search => (
                "memory_search",
                format!(
                    "Search saved memories by meaning and keywords. Current records override old \
                     statements. limit defaults to {MEMORY_RECALL_DEFAULT}, at most {MEMORY_RECALL_MAX}. \
                     If semantic search is unavailable the result explains the fallback."
                ),
                Effect::ReadOnly,
                json!({
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MEMORY_RECALL_MAX },
                }),
                vec!["query"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description,
            effect,
            input_schema: closed_object_schema(properties, &required),
        }
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(&ctx, &args).await {
            Ok(value) => Outcome::Ok { value },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl MemoryTool {
    async fn execute(
        &self,
        ctx: &CallContext,
        args: &Map<String, Value>,
    ) -> Result<Value, Failure> {
        let into_failure = |error: super::MemoryError| {
            failure(
                if matches!(error, super::MemoryError::Cancelled) {
                    code::CANCELLED
                } else {
                    code::TOOL_FAILED
                },
                error.to_string(),
            )
        };
        match self.kind {
            Kind::Save => {
                let row = self
                    .memories
                    .save(text(args)?, boolean(args, "pinned", false)?)
                    .await
                    .map_err(into_failure)?;
                Ok(json!({ "id": row.id }))
            }
            Kind::Update => {
                let id = id(args)?;
                if !args.contains_key("text") && !args.contains_key("pinned") {
                    return Err(invalid("provide text, pinned, or both"));
                }
                let text = if args.contains_key("text") {
                    Some(text(args)?)
                } else {
                    None
                };
                let pinned = if args.contains_key("pinned") {
                    Some(boolean(args, "pinned", false)?)
                } else {
                    None
                };
                let row = self
                    .memories
                    .update(id, text, pinned)
                    .await
                    .map_err(into_failure)?
                    .ok_or_else(|| {
                        failure(code::TOOL_FAILED, format!("memory {id} does not exist"))
                    })?;
                Ok(json!({ "id": row.id }))
            }
            Kind::Forget => {
                let forgotten = self
                    .memories
                    .forget(id(args)?)
                    .await
                    .map_err(into_failure)?;
                Ok(json!({ "forgotten": forgotten }))
            }
            Kind::Search => {
                let query = string(args, "query")?;
                let limit =
                    integer(args, "limit", MEMORY_RECALL_DEFAULT, 1, MEMORY_RECALL_MAX)? as usize;
                let recall = self
                    .memories
                    .recall(query, limit, &ctx.cancel)
                    .await
                    .map_err(into_failure)?;
                let rows: Vec<_> = recall
                    .memories
                    .into_iter()
                    .chain(recall.unindexed)
                    .map(|m| json!({ "id": m.id, "text": m.text, "pinned": m.pinned }))
                    .collect();
                let mut value = json!({ "memories": rows });
                if let Some(f) = recall.lexical_only {
                    value["note"] = json!(format!(
                        "semantic search unavailable ({}): {}; keyword recall used",
                        f.code, f.message
                    ));
                }
                Ok(value)
            }
        }
    }
}

fn id(args: &Map<String, Value>) -> Result<MemoryId, Failure> {
    string(args, "id")?
        .parse()
        .map_err(|e| invalid(format!("id: {e}")))
}

fn text(args: &Map<String, Value>) -> Result<String, Failure> {
    let text = string(args, "text")?.trim();
    if text.is_empty() || text.len() > MEMORY_TEXT_BYTES {
        return Err(invalid(format!(
            "text must be nonempty and at most {MEMORY_TEXT_BYTES} UTF-8 bytes"
        )));
    }
    Ok(text.into())
}
