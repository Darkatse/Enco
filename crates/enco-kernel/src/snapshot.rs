use crate::{
    ContextSource, Exports, KernelError, OfferedMemories, Tool, Tools, builtin::ActorCall,
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
    /// What answers a call to it.
    pub handler: Handler,
}

/// What answers a call to a Round's tool, given where the tool is defined.
#[derive(Clone)]
pub(crate) enum Handler {
    /// The Session actor, from the Session's own facts, when the call settles.
    Actor(ActorCall),
    /// A tool the host implements.
    Host(Arc<dyn Tool>),
    /// A plugin's tool, with the name and code the plugin ran under when the Round bound it.
    Plugin {
        name: PluginName,
        code: CodeRef,
        tools: Arc<dyn Tools>,
    },
}

impl SnapshotTool {
    /// The code a call to it runs, recorded when the call starts.
    pub fn code(&self) -> ToolCode {
        match &self.handler {
            Handler::Actor(_) => ToolCode::Native(crate::builtin::code()),
            Handler::Host(tool) => ToolCode::Native(tool.code()),
            Handler::Plugin { code, .. } => ToolCode::Plugin(*code),
        }
    }
}

impl Snapshot {
    /// The host's tools, then the kernel's own; each name is one tool, and the registry declared
    /// exactly these.
    pub fn new(
        node: NodeId,
        deps: &crate::KernelDeps,
        builtins: Vec<(ToolSpec, Handler)>,
    ) -> Result<Self, KernelError> {
        let mut names = HashSet::new();
        let mut tools = Vec::new();
        let host = deps
            .tools
            .iter()
            .map(|tool| (tool.spec(), Handler::Host(tool.clone())));
        for (spec, handler) in host.chain(builtins) {
            if !names.insert(spec.name.clone()) {
                return Err(KernelError::Config(format!("duplicate tool {}", spec.name)));
            }
            tools.push(SnapshotTool {
                id: CapabilityId {
                    node,
                    name: spec.name.clone(),
                },
                spec,
                handler,
            });
        }
        let declared: std::collections::BTreeSet<_> =
            deps.registry.host_tool_names().iter().collect();
        let actual: std::collections::BTreeSet<_> =
            tools.iter().map(|tool| &tool.spec.name).collect();
        if declared != actual {
            return Err(KernelError::Config(format!(
                "host tool declarations differ from handlers: missing handlers {:?}; undeclared handlers {:?}",
                declared.difference(&actual).collect::<Vec<_>>(),
                actual.difference(&declared).collect::<Vec<_>>()
            )));
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
            snapshot.tools.push(SnapshotTool {
                id: CapabilityId {
                    node: self.node,
                    name: spec.name.clone(),
                },
                spec,
                handler: Handler::Plugin {
                    name: name.clone(),
                    code: export.code,
                    tools: export.adapter,
                },
            });
        }
        snapshot
    }
}
