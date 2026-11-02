//! `message_send`: record a message or file for the current reply destination.
use super::{failure::*, resolve_path};
use crate::limits::MESSAGE_FILE_MAX_BYTES;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::Path;
use tokio::io::AsyncReadExt;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MessageArguments {
    /// The message in Markdown.
    text: Option<String>,
    #[schemars(description = format!(
        "A file to send, relative to the working directory or absolute; at most {} MB.",
        MESSAGE_FILE_MAX_BYTES / 1_000_000
    ))]
    file: Option<String>,
}

pub(super) struct MessageSend {
    node: NodeId,
    spec: ToolSpec,
}

impl MessageSend {
    pub(super) fn new(node: NodeId) -> Result<Self, SchemaError> {
        Ok(Self {
            node,
            spec: ToolSpec::of::<MessageArguments>(
                "message_send",
                "Send a message now to the one you are currently replying to: the owner, or the Session that delegated to you. \
                 Send Markdown text, one file, or both. \
                 Your final reply is delivered normally; do not repeat it here. \
                 Text accompanying tool calls may not reach them. \
                 Success confirms the message was recorded for delivery; channel delivery failures \
                 appear in the channels section of self_status.",
                Effect::ReadOnly,
            )?,
        })
    }
}

#[async_trait]
impl Tool for MessageSend {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.prepare(&ctx, args).await {
            Ok(outgoing) => Outcome::Ok {
                value: "Message recorded for delivery to the one you are currently replying to."
                    .into(),
                images: Vec::new(),
                outgoing: Some(outgoing),
            },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl MessageSend {
    async fn prepare(
        &self,
        ctx: &CallContext,
        args: Map<String, Value>,
    ) -> Result<Outgoing<FileData>, Failure> {
        let MessageArguments { text, file } = parse_arguments(args)?;
        let text = text.unwrap_or_default();
        if text.trim().is_empty() && file.is_none() {
            return Err(invalid("provide nonempty text or a file"));
        }
        let file = match file {
            Some(file) => {
                Some(read(&resolve_path(self.node, &ctx.working_dir, &file).await?).await?)
            }
            None => None,
        };
        Ok(Outgoing { text, file })
    }
}

/// Reading one byte past the limit bounds memory even if the file grows meanwhile.
async fn read(path: &Path) -> Result<FileData, Failure> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("file must name a file"))?
        .to_string_lossy()
        .into_owned();
    let io = |error| failure(code::TOOL_FAILED, format!("{}: {error}", path.display()));
    let mut data = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(io)?
        .take(MESSAGE_FILE_MAX_BYTES + 1)
        .read_to_end(&mut data)
        .await
        .map_err(io)?;
    if data.len() as u64 > MESSAGE_FILE_MAX_BYTES {
        return Err(failure(
            code::TOOL_FAILED,
            format!(
                "{}: file exceeds {} MB",
                path.display(),
                MESSAGE_FILE_MAX_BYTES / 1_000_000
            ),
        ));
    }
    Ok(FileData { name, data })
}
