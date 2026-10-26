use crate::{CallContext, ContextSource, Exports, KernelError, OfferedMemories, Tool, Tools};
use enco_core::*;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone)]
pub(crate) struct Snapshot {
    node: NodeId,
    pub instructions: Arc<dyn ContextSource<Supply = Option<String>>>,
    pub memories: Arc<dyn ContextSource<Supply = OfferedMemories>>,
    pub tools: Vec<SnapshotTool>,
    pub lifeline: Vec<CapabilityId>,
    pub unavailable: Vec<(String, Failure)>,
}

#[derive(Clone)]
pub(crate) struct SnapshotTool {
    pub id: CapabilityId,
    pub spec: ToolSpec,
    pub code: CodeRef,
    pub tool: Arc<dyn Tool>,
}

impl Snapshot {
    pub fn new(node: NodeId, deps: &crate::KernelDeps) -> Result<Self, KernelError> {
        let mut names = HashSet::new();
        let mut tools = Vec::new();
        for tool in &deps.tools {
            let spec = tool.spec();
            if !names.insert(spec.name.clone()) {
                return Err(KernelError::Config(format!("duplicate tool {}", spec.name)));
            }
            tools.push(SnapshotTool {
                id: CapabilityId {
                    node,
                    name: spec.name.clone(),
                },
                spec,
                code: tool.code(),
                tool: tool.clone(),
            });
        }
        let lifeline = deps
            .lifeline
            .iter()
            .map(|name| {
                tools
                    .iter()
                    .find(|t| t.id.name == *name)
                    .map(|t| t.id.clone())
                    .ok_or_else(|| {
                        KernelError::Config(format!("lifeline tool {name} does not exist"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            node,
            instructions: deps.instructions.clone(),
            memories: deps.memories.clone(),
            tools,
            lifeline,
            unavailable: Vec::new(),
        })
    }

    /// Freeze the published plugin tools alongside the host's capabilities for this Round.
    pub fn for_round(&self, exports: &Exports, safe_mode: bool) -> Self {
        let mut snapshot = self.clone();
        snapshot.unavailable = exports.unavailable();
        if safe_mode {
            return snapshot;
        }
        for (spec, export) in exports.tools() {
            let tool = Arc::new(GenerationTool {
                spec,
                generation: export.generation,
                adapter: export.adapter,
            });
            snapshot.tools.push(SnapshotTool {
                id: CapabilityId {
                    node: self.node,
                    name: tool.spec.name.clone(),
                },
                spec: tool.spec(),
                code: tool.code(),
                tool,
            });
        }
        snapshot
    }
}

/// Registry identity wraps the runtime's tools interface at the ordinary Tool boundary.
struct GenerationTool {
    spec: ToolSpec,
    generation: GenerationId,
    adapter: Arc<dyn Tools>,
}

#[async_trait::async_trait]
impl Tool for GenerationTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }
    fn code(&self) -> CodeRef {
        CodeRef::Generation {
            id: self.generation,
        }
    }
    async fn call(
        &self,
        ctx: CallContext,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        self.adapter.call(ctx, &self.spec, args).await
    }
}
