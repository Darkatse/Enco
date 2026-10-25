use super::Memories;
use crate::{limits::*, tools::failure::*};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Kind {
    Save,
    Update,
    Forget,
    Search,
}

fn statement_limit() -> String {
    format!("at most {MEMORY_TEXT_BYTES} UTF-8 bytes")
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SaveArguments {
    #[schemars(description = format!("The statement to save, {}.", statement_limit()))]
    text: String,
    /// Whether to pin it, so it appears in every conversation.
    #[serde(default)]
    pinned: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateArguments {
    /// The memory to correct.
    id: MemoryId,
    #[schemars(description = format!("The corrected statement, {}.", statement_limit()))]
    text: Option<String>,
    /// Whether it is pinned, so it appears in every conversation.
    pinned: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ForgetArguments {
    /// The memory to delete.
    id: MemoryId,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    /// What to look for.
    query: String,
    /// The most memories to return.
    #[serde(default = "default_recall")]
    #[schemars(range(min = 1, max = MEMORY_RECALL_MAX))]
    limit: u64,
}

fn default_recall() -> u64 {
    MEMORY_RECALL_DEFAULT
}

impl Kind {
    const ALL: [Self; 4] = [Self::Save, Self::Update, Self::Forget, Self::Search];
    fn spec(self) -> Result<ToolSpec, SchemaError> {
        match self {
            Kind::Save => ToolSpec::of::<SaveArguments>(
                "memory_save",
                "Save a durable fact about the owner as one self-contained statement. Include dates \
                 when time-bound. Pin only facts useful in almost every conversation.",
                Effect::SideEffect,
            ),
            Kind::Update => ToolSpec::of::<UpdateArguments>(
                "memory_update",
                "Correct a saved memory by its id. Supply text, pinned, or both; correction replaces \
                 the old statement.",
                Effect::SideEffect,
            ),
            Kind::Forget => ToolSpec::of::<ForgetArguments>(
                "memory_forget",
                "Delete a memory from the current memory authority. Historical conversation logs are unchanged.",
                Effect::SideEffect,
            ),
            Kind::Search => ToolSpec::of::<SearchArguments>(
                "memory_search",
                "Search saved memories by meaning and keywords. Current records override old \
                 statements. If semantic search is unavailable the result explains the fallback.",
                Effect::ReadOnly,
            ),
        }
    }
}

/// Native declarations available before their stateful handlers are constructed.
pub fn memory_tool_specs() -> Result<Vec<ToolSpec>, SchemaError> {
    Kind::ALL.into_iter().map(Kind::spec).collect()
}

struct MemoryTool {
    memories: Arc<Memories>,
    kind: Kind,
    spec: ToolSpec,
}

pub fn memory_tools(memories: Arc<Memories>) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            Ok(Arc::new(MemoryTool {
                memories: memories.clone(),
                spec: kind.spec()?,
                kind,
            }) as Arc<dyn Tool>)
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
        self.spec.clone()
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(&ctx, args).await {
            Ok(value) => Outcome::Ok {
                value,
                images: vec![],
                outgoing: None,
            },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl MemoryTool {
    async fn execute(&self, ctx: &CallContext, args: Map<String, Value>) -> Result<Value, Failure> {
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
                let SaveArguments { text, pinned } = parse_arguments(args)?;
                let row = self
                    .memories
                    .save(statement(&text)?, pinned)
                    .await
                    .map_err(into_failure)?;
                Ok(json!({ "id": row.id }))
            }
            Kind::Update => {
                let UpdateArguments { id, text, pinned } = parse_arguments(args)?;
                if text.is_none() && pinned.is_none() {
                    return Err(invalid("provide text, pinned, or both"));
                }
                let text = text.as_deref().map(statement).transpose()?;
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
                let ForgetArguments { id } = parse_arguments(args)?;
                let forgotten = self.memories.forget(id).await.map_err(into_failure)?;
                Ok(json!({ "forgotten": forgotten }))
            }
            Kind::Search => {
                let SearchArguments { query, limit } = parse_arguments(args)?;
                let recall = self
                    .memories
                    .recall(&query, limit as usize, &ctx.cancel)
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

/// A statement as stored: trimmed, nonempty and within the byte limit.
fn statement(text: &str) -> Result<String, Failure> {
    let text = text.trim();
    if text.is_empty() || text.len() > MEMORY_TEXT_BYTES {
        return Err(invalid(format!(
            "text must be nonempty and {}",
            statement_limit()
        )));
    }
    Ok(text.into())
}
