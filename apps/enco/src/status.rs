use crate::compose_root::Application;
use async_trait::async_trait;
use enco_core::{CodeRef, Failure, Outcome, ToolSpec, code};
use enco_host::development::Failed;
use enco_kernel::{CallContext, KernelError, Tool};
use serde_json::{Map, Value};
use std::sync::{OnceLock, Weak};

/// Each source keeps its own error code.
#[derive(Debug)]
pub(crate) enum ReadError {
    Development(Failed),
    Kernel(KernelError),
}

impl From<Failed> for ReadError {
    fn from(failed: Failed) -> Self {
        Self::Development(failed)
    }
}

impl From<KernelError> for ReadError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Development(failed) => failed.fmt(f),
            Self::Kernel(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ReadError {}

impl ReadError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Development(failed) => failed.code(),
            Self::Kernel(_) => code::TOOL_FAILED,
        }
    }
}

pub(crate) async fn read(
    application: &Application,
) -> Result<enco_host::status::Status, ReadError> {
    let development = application
        .development
        .status(application.kernel.registry())
        .await?;
    let execution = application.kernel.status().await?;
    Ok(enco_host::status::Status {
        development,
        node: enco_host::status::Node {
            version: env!("CARGO_PKG_VERSION").into(),
            revision: env!("ENCO_REVISION").into(),
            home: application.paths.home.clone(),
            execution,
            channels: application.channels.status().await?,
        },
    })
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
    async fn call(&self, _ctx: CallContext, arguments: Map<String, Value>) -> Outcome {
        if let Err(failure) = enco_host::development::StatusArguments::parse(arguments) {
            return Outcome::Failed { failure };
        }
        let Some(application) = self.application.get().and_then(Weak::upgrade) else {
            return Outcome::Failed {
                failure: Failure {
                    code: code::TOOL_FAILED.into(),
                    message: "application is not running".into(),
                    retryable: false,
                },
            };
        };
        match read(&application).await {
            Ok(status) => Outcome::Ok {
                value: status.document().into(),
                images: Vec::new(),
            },
            Err(error) => Outcome::Failed {
                failure: Failure {
                    code: error.code().into(),
                    message: error.to_string(),
                    retryable: false,
                },
            },
        }
    }
}
