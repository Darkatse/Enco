use super::args;
use crate::{
    development::{Development, DevelopmentError},
    limits::BUILD_DIAGNOSTICS,
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Registry, RegistryError, Tool};
use serde_json::{Map, Value, json};
use std::sync::Arc;

enum Kind {
    Apply,
    Scaffold,
    Rename,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Apply, Self::Scaffold, Self::Rename];

    fn spec(&self) -> ToolSpec {
        let (name, description, properties, required) = match self {
            Self::Apply => (
                "self_apply",
                format!("Build and register changed plugin sources and config.toml together. Edit files in the Enco home first. Registration does not commit your branch: its history is enco/registered. Use self_status for unregistered intent, since Git also shows text-only changes such as comments. To undo a file change, find its registration with git log enco/registered -- <path>, restore that path from the registration's parent commit, then call self_apply. Remove an untracked directory yourself when undoing its creation. Any semantic change to config.toml accepts the whole document; restore the running document before editing when preserving a configuration rollback. Registration validates the current contract and environment and rejects invalid changes. New code remains on trial until healthy; automatic recovery does not change registered intent. Return build/reuse evidence, probe results, the first {BUILD_DIAGNOSTICS} compiler diagnostics and the complete log path; registered is null when no intent changed. A concurrent registry change rejects the attempt; inspect self_status and retry. Post-commit cleanup warnings accompany the accepted result."),
                json!({}),
                vec![],
            ),
            Self::Scaffold => (
                "plugin_scaffold",
                "Create a standalone tool plugin under plugins/<name> in the Enco home. Writes source, Cargo configuration and README without registering or activating it. Edit the source, then call self_apply.".into(),
                json!({ "name": { "type": "string" } }),
                vec!["name"],
            ),
            Self::Rename => (
                "plugin_rename",
                "Move a plugin's source directory and register its new name, preserving identity, code, settings and unregistered working edits. Factory names and names still referenced by the active configuration cannot be renamed. Use this command instead of git mv.".into(),
                json!({ "name": { "type": "string" }, "to": { "type": "string" } }),
                vec!["name", "to"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description,
            effect: Effect::SideEffect,
            input_schema: closed_object_schema(properties, &required),
        }
    }
}

/// Declaration shared by the application's status tool and its native command.
pub fn self_status_spec() -> ToolSpec {
    ToolSpec {
        name: "self_status".into(),
        description: "Inspect the same state as enco status: host version and source commit, Sessions, channels, registrations, active generations, rollback targets and failures. Compare working files with registered intent separately from running generations. The accepted chain commit and its Git reference are reported separately. Source and configuration history identify the changes whose parent commits restore earlier file intent; settings generations identify the earlier generation that supplied their code. Status never builds source.".into(),
        input_schema: closed_object_schema(json!({}), &[]),
        effect: Effect::ReadOnly,
    }
}

/// Declarations are available before the registry binds their handlers.
pub fn development_tool_specs() -> Vec<ToolSpec> {
    Kind::ALL.iter().map(Kind::spec).collect()
}

/// File operations and registration follow the same path used by the CLI.
pub fn development_tools(
    development: Arc<Development>,
    registry: Arc<Registry>,
) -> Vec<Arc<dyn Tool>> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            Arc::new(DevelopmentTool {
                development: development.clone(),
                registry: registry.clone(),
                kind,
            }) as Arc<dyn Tool>
        })
        .collect()
}

struct DevelopmentTool {
    development: Arc<Development>,
    registry: Arc<Registry>,
    kind: Kind,
}

#[async_trait]
impl Tool for DevelopmentTool {
    fn code(&self) -> CodeRef {
        super::code()
    }
    fn spec(&self) -> ToolSpec {
        self.kind.spec()
    }

    async fn call(&self, ctx: CallContext, arguments: Map<String, Value>) -> Outcome {
        match self.execute(ctx, arguments).await {
            Ok(value) => Outcome::Ok { value },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl DevelopmentTool {
    async fn execute(
        &self,
        ctx: CallContext,
        arguments: Map<String, Value>,
    ) -> Result<Value, Failure> {
        Ok(match self.kind {
            Kind::Apply => json!(
                self.development
                    .apply(&self.registry, &ctx.cancel)
                    .await
                    .map_err(failure)?
            ),
            Kind::Scaffold => {
                json!({ "files": self.development.scaffold(&self.registry, args::string(&arguments, "name")?).await.map_err(failure)? })
            }
            Kind::Rename => json!(
                self.development
                    .rename(
                        &self.registry,
                        args::string(&arguments, "name")?,
                        args::string(&arguments, "to")?,
                        &ctx.cancel
                    )
                    .await
                    .map_err(failure)?
            ),
        })
    }
}

fn failure(error: DevelopmentError) -> Failure {
    let code = match &error {
        DevelopmentError::Cancelled => code::CANCELLED,
        DevelopmentError::Invalid(_)
        | DevelopmentError::Registry(RegistryError::UnknownPlugin(_)) => {
            code::TOOL_INVALID_ARGUMENTS
        }
        DevelopmentError::Registry(RegistryError::Rejected { .. } | RegistryError::Conflict(_)) => {
            code::PLUGIN_REJECTED
        }
        _ => code::TOOL_FAILED,
    };
    args::failure(code, error.to_string())
}
