use super::failure;
pub use crate::development::self_status_spec;
use crate::development::{Development, DevelopmentCall, DevelopmentTool, Failed};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Registry, Tool};
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// Declarations are available before the registry binds their handlers.
pub fn development_tool_specs() -> Result<Vec<ToolSpec>, SchemaError> {
    DevelopmentTool::ALL
        .into_iter()
        .map(DevelopmentTool::spec)
        .collect()
}

/// File operations and registration follow the same path used by the CLI.
pub fn development_tools(
    development: Arc<Development>,
    registry: Arc<Registry>,
) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    DevelopmentTool::ALL
        .into_iter()
        .map(|tool| {
            Ok(Arc::new(DevelopmentHandler {
                development: development.clone(),
                registry: registry.clone(),
                spec: tool.spec()?,
                tool,
            }) as Arc<dyn Tool>)
        })
        .collect()
}

struct DevelopmentHandler {
    development: Arc<Development>,
    registry: Arc<Registry>,
    tool: DevelopmentTool,
    spec: ToolSpec,
}

#[async_trait]
impl Tool for DevelopmentHandler {
    fn code(&self) -> CodeRef {
        super::code()
    }
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }
    async fn call(&self, ctx: CallContext, arguments: Map<String, Value>) -> Outcome {
        let result = match self.tool.parse(arguments) {
            Ok(call) => self.execute(ctx, call).await.map_err(failure),
            Err(failure) => Err(failure),
        };
        match result {
            Ok(value) => Outcome::Ok {
                value,
                images: vec![],
                outgoing: None,
            },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl DevelopmentHandler {
    async fn execute(&self, ctx: CallContext, call: DevelopmentCall) -> Result<Value, Failed> {
        let agent = |reason| Initiator::Agent {
            session: ctx.session,
            call: ctx.call,
            reason,
        };
        let development = &self.development;
        let registry = &self.registry;
        Ok(match call {
            DevelopmentCall::Apply(args) => development
                .apply(registry, agent(args.reason), &ctx.cancel)
                .await?
                .document()
                .into(),
            DevelopmentCall::History(query) => development
                .history(registry, query)
                .await?
                .document()
                .into(),
            DevelopmentCall::Rewind(args) => development
                .rewind(registry, args, &ctx.cancel)
                .await?
                .document()
                .into(),
            DevelopmentCall::Scaffold(args) => {
                json!({"files": development.scaffold(registry, &args.name).await?})
            }
            DevelopmentCall::Rename(args) => crate::development::registration_document(
                &development
                    .rename(
                        registry,
                        &args.name,
                        &args.to,
                        agent(args.reason),
                        &ctx.cancel,
                    )
                    .await?,
            )
            .into(),
        })
    }
}

fn failure(failed: Failed) -> Failure {
    failure::failure(failed.code(), failed.to_string())
}
