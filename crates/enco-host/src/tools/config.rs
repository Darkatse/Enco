use super::args;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{Applied, CallContext, ConfigSource, Registry, RegistryError, Tool};
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Intent file failures belong to the host filesystem boundary.
#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid {}: {reason}", path.display())]
    Invalid { path: PathBuf, reason: String },
}

/// Reading intent and registering it are distinct operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigApplyError {
    #[error(transparent)]
    File(#[from] ConfigFileError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

/// Read intent without interpreting or removing any configuration tables.
pub async fn document(path: &Path) -> Result<Value, ConfigFileError> {
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|source| ConfigFileError::Read {
            path: path.into(),
            source,
        })?;
    let invalid = |reason: String| ConfigFileError::Invalid {
        path: path.into(),
        reason,
    };
    let value: toml::Value =
        toml::from_str(&text).map_err(|error: toml::de::Error| invalid(error.to_string()))?;
    serde_json::to_value(value).map_err(|error| invalid(error.to_string()))
}

/// CLI and model tools supply the same content to the sole registry committer.
pub async fn apply(
    path: &Path,
    registry: &Registry,
    generation: Option<GenerationId>,
) -> Result<Value, ConfigApplyError> {
    let source = match generation {
        Some(id) => ConfigSource::Generation(id),
        None => ConfigSource::Document(document(path).await?),
    };
    Ok(match registry.apply_config(source).await? {
        Applied::Registered {
            generation,
            previous,
            probes,
        } => {
            json!({ "generation": generation, "previous": previous, "unchanged": false, "probes": probes })
        }
        Applied::Unchanged(generation) => {
            json!({ "generation": generation, "previous": generation, "unchanged": true, "probes": [] })
        }
    })
}

pub fn tools(path: PathBuf, registry: Arc<Registry>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(ConfigApply { path, registry })]
}

struct ConfigApply {
    path: PathBuf,
    registry: Arc<Registry>,
}

#[async_trait]
impl Tool for ConfigApply {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "config_apply".into(),
            description: format!(
                "Edit {} then apply it, probing newly used model endpoints with a fixed prompt. Changes commit as one configuration generation. Supply the previous generation number to undo changes by registering its content again. This leaves config.toml unchanged: restore that file too, or applying it again will reintroduce the undone changes. A conflict means another registry change won; inspect status and retry.",
                self.path.display()
            ),
            input_schema: closed_object_schema(
                json!({"generation": {"type": "integer", "minimum": 1}}),
                &[],
            ),
            effect: Effect::SideEffect,
        }
    }

    async fn call(&self, _context: CallContext, args: Map<String, Value>) -> Outcome {
        let generation = match args.get("generation") {
            None => None,
            Some(value) => match value.as_u64().filter(|id| *id > 0) {
                Some(id) => Some(GenerationId(id)),
                None => {
                    return Outcome::Failed {
                        failure: args::invalid("generation must be a positive integer"),
                    };
                }
            },
        };
        match apply(&self.path, &self.registry, generation).await {
            Ok(value) => Outcome::Ok { value },
            Err(error) => {
                let code = match &error {
                    ConfigApplyError::Registry(RegistryError::UnknownGeneration(_)) => {
                        code::TOOL_INVALID_ARGUMENTS
                    }
                    ConfigApplyError::File(_)
                    | ConfigApplyError::Registry(
                        RegistryError::Rejected { .. } | RegistryError::Conflict(_),
                    ) => code::CONFIG_REJECTED,
                    _ => code::TOOL_FAILED,
                };
                Outcome::Failed {
                    failure: args::failure(code, error.to_string()),
                }
            }
        }
    }
}
