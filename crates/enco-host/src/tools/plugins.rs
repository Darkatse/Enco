use super::{args, resolve_path};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Registry, RegistryError, TRIAL_CALLS, Tool};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc};

enum Kind {
    Status,
    Deploy,
    Rollback,
}
struct PluginTool {
    registry: Arc<Registry>,
    workspace: PathBuf,
    kind: Kind,
}

/// Plugin commands use the same registry entry points as the native CLI.
pub fn plugin_tools(registry: Arc<Registry>, workspace: PathBuf) -> Vec<Arc<dyn Tool>> {
    [Kind::Status, Kind::Deploy, Kind::Rollback]
        .into_iter()
        .map(|kind| {
            Arc::new(PluginTool {
                registry: registry.clone(),
                workspace: workspace.clone(),
                kind,
            }) as Arc<dyn Tool>
        })
        .collect()
}

#[async_trait]
impl Tool for PluginTool {
    fn code(&self) -> CodeRef {
        super::code()
    }
    fn spec(&self) -> ToolSpec {
        let (name, description, effect, properties, required) = match self.kind {
            Kind::Status => (
                "plugin_status",
                "Inspect registered plugins, active generations, failure reasons, rollback targets and configured users.".into(),
                Effect::ReadOnly,
                json!({}),
                vec![],
            ),
            Kind::Deploy => (
                "plugin_deploy",
                format!("Deploy a WebAssembly component after its local self-check. path is relative to the workspace unless absolute. New calls use the trial generation immediately; {TRIAL_CALLS} successful calls make it healthy. A plugin trap or contract violation during trial automatically rolls back to an earlier healthy generation. Service failures do not affect plugin health. Inspect with plugin_status; use plugin_rollback for manual recovery."),
                Effect::SideEffect,
                json!({"name": {"type": "string"}, "path": {"type": "string"}}),
                vec!["name", "path"],
            ),
            Kind::Rollback => (
                "plugin_rollback",
                "Activate the nearest earlier usable healthy generation of a registered plugin. The current generation is preserved if no usable target exists. A conflict means the registry changed while the rollback was prepared; inspect status before retrying.".into(),
                Effect::SideEffect,
                json!({"name": {"type": "string"}}),
                vec!["name"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description,
            effect,
            input_schema: closed_object_schema(properties, &required),
        }
    }

    async fn call(&self, _ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.execute(args).await {
            Ok(value) => Outcome::Ok { value },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl PluginTool {
    async fn execute(&self, args: Map<String, Value>) -> Result<Value, Failure> {
        Ok(match self.kind {
            Kind::Status => json!(self.registry.status().await),
            Kind::Deploy => {
                let name = args::string(&args, "name")?;
                let path = resolve_path(&self.workspace, args::string(&args, "path")?);
                let bytes = tokio::fs::read(&path).await.map_err(|error| {
                    args::failure(code::TOOL_FAILED, format!("{}: {error}", path.display()))
                })?;
                json!(
                    self.registry
                        .deploy(name, bytes)
                        .await
                        .map_err(registry_error)?
                )
            }
            Kind::Rollback => json!(
                self.registry
                    .rollback(args::string(&args, "name")?)
                    .await
                    .map_err(registry_error)?
            ),
        })
    }
}

fn registry_error(error: RegistryError) -> Failure {
    let code = match &error {
        RegistryError::UnknownPlugin(_) => code::TOOL_INVALID_ARGUMENTS,
        RegistryError::Rejected { .. }
        | RegistryError::NoRollbackTarget(_)
        | RegistryError::Conflict(_) => code::PLUGIN_REJECTED,
        _ => code::TOOL_FAILED,
    };
    args::failure(code, error.to_string())
}
