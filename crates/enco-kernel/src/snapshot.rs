use crate::{Composer, ContextSource, KernelError, Tool};
use enco_core::*;
use std::{collections::HashSet, sync::Arc};

pub(crate) struct Snapshot {
    pub composer: Arc<dyn Composer>,
    pub context: Vec<Arc<dyn ContextSource>>,
    pub tools: Vec<SnapshotTool>,
    pub lifeline: Vec<CapabilityId>,
}

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
            composer: deps.composer.clone(),
            context: deps.context.clone(),
            tools,
            lifeline,
        })
    }
}
