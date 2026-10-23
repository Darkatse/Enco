use super::{args::*, resolve_path};
use crate::limits::MESSAGE_FILE_MAX_BYTES;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

pub(super) struct MessageSend {
    pub workspace: PathBuf,
}

#[async_trait]
impl Tool for MessageSend {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "message_send".into(),
            description: format!(
                "Send the owner a message now, with Markdown text, one file, or both. \
                 Relative file paths use the workspace; absolute paths are allowed. \
                 A file may be at most {} MB. \
                 Your final reply is delivered normally; do not repeat it here. \
                 Text accompanying tool calls may not reach the owner. \
                 Success confirms the message was recorded for delivery; delivery failures \
                 appear in the channels section of self_status.",
                MESSAGE_FILE_MAX_BYTES / 1_000_000
            ),
            input_schema: closed_object_schema(
                json!({"text": {"type": "string"}, "file": {"type": "string"}}),
                &[],
            ),
            effect: Effect::ReadOnly,
        }
    }

    async fn call(&self, _ctx: CallContext, args: Map<String, Value>) -> Outcome {
        match self.prepare(&args).await {
            Ok(outgoing) => Outcome::Ok {
                value: "Message recorded for delivery to the owner.".into(),
                images: Vec::new(),
                outgoing: Some(outgoing),
            },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl MessageSend {
    async fn prepare(&self, args: &Map<String, Value>) -> Result<Outgoing<FileData>, Failure> {
        let text = optional_string(args, "text")?.unwrap_or_default();
        let file = optional_string(args, "file")?;
        if text.trim().is_empty() && file.is_none() {
            return Err(invalid("provide nonempty text or a file"));
        }
        let file = match file {
            Some(file) => Some(read(&resolve_path(&self.workspace, file)).await?),
            None => None,
        };
        Ok(Outgoing {
            text: text.to_owned(),
            file,
        })
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
