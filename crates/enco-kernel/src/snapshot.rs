use crate::{
    CallContext, ContextSource, Exports, KernelError, OfferedMemories, Tool, Tools,
    builtin::BuiltinCall,
};
use enco_core::*;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone)]
pub(crate) struct Snapshot {
    node: NodeId,
    pub owner_instructions: Arc<dyn ContextSource<Supply = Option<String>>>,
    pub profile_instructions: Arc<dyn ContextSource<Supply = Option<String>>>,
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
    /// Whose tool this is.
    pub source: ToolSource,
}

/// Where a Round's tool comes from.
#[derive(Clone)]
pub(crate) enum ToolSource {
    /// The host's own, with the built-in call it is when the Session actor answers it.
    Host(Option<BuiltinCall>),
    /// A plugin's, by the name the plugin ran under when the Round bound it; only plugin code can
    /// be recorded as failed.
    Plugin(PluginName),
}

impl SnapshotTool {
    /// The built-in call this tool is, when the Session actor answers it.
    pub fn builtin(&self) -> Option<BuiltinCall> {
        match &self.source {
            ToolSource::Host(builtin) => *builtin,
            ToolSource::Plugin(_) => None,
        }
    }
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
                source: ToolSource::Host(BuiltinCall::named(&spec.name)),
                spec,
                code: tool.code(),
                tool: tool.clone(),
            });
        }
        let lifeline = deps
            .registry
            .lifeline()
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
            owner_instructions: deps.owner_instructions.clone(),
            profile_instructions: deps.profile_instructions.clone(),
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
        for (name, spec, export) in exports.tools() {
            let tool = Arc::new(PluginTool {
                spec,
                code: export.code,
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
                source: ToolSource::Plugin(name.clone()),
            });
        }
        snapshot
    }
}

/// Registry identity wraps the runtime's tools interface at the ordinary Tool boundary.
struct PluginTool {
    spec: ToolSpec,
    code: CodeRef,
    adapter: Arc<dyn Tools>,
}

#[async_trait::async_trait]
impl Tool for PluginTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }
    fn code(&self) -> CodeRef {
        self.code.clone()
    }
    async fn call(
        &self,
        ctx: CallContext,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        self.adapter.call(ctx, &self.spec, args).await
    }
}
