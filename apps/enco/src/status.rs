use crate::compose_root::Application;
use async_trait::async_trait;
use enco_core::{CodeRef, Failure, Outcome, ToolSpec, code};
use enco_kernel::{CallContext, KernelError, Tool};
use serde_json::{Map, Value, json};
use std::sync::{OnceLock, Weak};

pub(crate) async fn read(application: &Application) -> Result<Value, KernelError> {
    let (registry, workspace) = application
        .development
        .status(application.kernel.registry())
        .await?;
    let mut status = json!(application.kernel.status().await?);
    status["host"] = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "revision": env!("ENCO_REVISION"),
        "home": application.paths.home,
    });
    status["channels"] = json!(application.channels.status().await?);
    status["config"] = json!(registry.config);
    status["plugins"] = json!(registry.plugins);
    status["registrations"] = json!(registry.registrations);
    status["registered"] = json!(registry.registered);
    status["workspace"] = json!(workspace);
    Ok(status)
}

/// The application binds its status tool after construction; CLI and tools share `read`.
#[derive(Default)]
pub(crate) struct StatusTool {
    pub application: OnceLock<Weak<Application>>,
}

#[async_trait]
impl Tool for StatusTool {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "application".into(),
            version: env!("ENCO_VERSION").into(),
        }
    }
    fn spec(&self) -> ToolSpec {
        enco_host::self_status_spec()
    }

    async fn call(&self, _ctx: CallContext, _arguments: Map<String, Value>) -> Outcome {
        let application = self.application.get().and_then(Weak::upgrade);
        let result = match application {
            Some(application) => read(&application).await.map_err(|error| error.to_string()),
            None => Err("application is not running".into()),
        };
        match result {
            Ok(value) => Outcome::Ok { value },
            Err(error) => Outcome::Failed {
                failure: Failure {
                    code: code::TOOL_FAILED.into(),
                    message: error.to_string(),
                    retryable: false,
                },
            },
        }
    }
}
