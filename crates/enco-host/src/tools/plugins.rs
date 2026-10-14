use super::{args, resolve_path};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, PluginSource, Registry, RegistryError, TRIAL_CALLS, Tool};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc};

enum Kind {
    Status,
    Deploy,
}
impl Kind {
    const ALL: [Self; 2] = [Self::Status, Self::Deploy];
    fn spec(&self) -> ToolSpec {
        let (name, description, effect, properties, required) = match self {
            Kind::Status => (
                "plugin_status",
                "Inspect registered plugins, active generations, failure reasons, rollback targets, configured users and differences between active settings and registered intent.".into(),
                Effect::ReadOnly,
                json!({}),
                vec![],
            ),
            Kind::Deploy => (
                "plugin_deploy",
                format!("Deploy a WebAssembly component after its local self-check. path is relative to the workspace unless absolute. Tool definitions and implementations change at the next Round; completion adapters are selected for each Attempt. {TRIAL_CALLS} successful calls make the trial generation healthy. A plugin trap or contract violation during trial automatically rolls back to an earlier healthy generation. Service failures do not affect plugin health. Provide exactly one of path or generation. A path keeps the active settings; a generation restores its code, configuration and secret mappings together, without rewriting plugin source or config.toml. Inspect history with plugin_status."),
                Effect::SideEffect,
                json!({"name": {"type": "string"}, "path": {"type": "string"}, "generation": {"type": "integer", "minimum": 1}}),
                vec!["name"],
            ),
        };
        let mut input_schema = closed_object_schema(properties, &required);
        if matches!(self, Kind::Deploy) {
            input_schema["oneOf"] = json!([{"required": ["path"]}, {"required": ["generation"]}]);
        }
        ToolSpec {
            name: name.into(),
            description,
            effect,
            input_schema,
        }
    }
}

/// Native declarations available before their stateful handlers are constructed.
pub fn plugin_tool_specs() -> Vec<ToolSpec> {
    Kind::ALL.iter().map(Kind::spec).collect()
}

struct PluginTool {
    registry: Arc<Registry>,
    workspace: PathBuf,
    kind: Kind,
}

/// Plugin commands use the same registry entry points as the native CLI.
pub fn plugin_tools(registry: Arc<Registry>, workspace: PathBuf) -> Vec<Arc<dyn Tool>> {
    Kind::ALL
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
        self.kind.spec()
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
            Kind::Status => json!(self.registry.status().await.plugins),
            Kind::Deploy => {
                let name = args::string(&args, "name")?;
                let source = match (args.get("path"), args.get("generation")) {
                    (Some(_), None) => {
                        let path = resolve_path(&self.workspace, args::string(&args, "path")?);
                        PluginSource::Artifact(tokio::fs::read(&path).await.map_err(|error| {
                            args::failure(code::TOOL_FAILED, format!("{}: {error}", path.display()))
                        })?)
                    }
                    (None, Some(_)) => PluginSource::Generation(GenerationId(args::integer(
                        &args,
                        "generation",
                        1,
                        1,
                        u64::MAX,
                    )?)),
                    _ => return Err(args::invalid("provide exactly one of path or generation")),
                };
                json!(
                    self.registry
                        .deploy(name, source)
                        .await
                        .map_err(registry_error)?
                )
            }
        })
    }
}

fn registry_error(error: RegistryError) -> Failure {
    let code = match &error {
        RegistryError::UnknownPlugin(_) | RegistryError::UnknownGeneration(_) => {
            code::TOOL_INVALID_ARGUMENTS
        }
        RegistryError::Rejected { .. }
        | RegistryError::NoRollbackTarget(_)
        | RegistryError::Conflict(_) => code::PLUGIN_REJECTED,
        _ => code::TOOL_FAILED,
    };
    args::failure(code, error.to_string())
}
