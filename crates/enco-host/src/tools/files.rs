use super::{args::*, resolve_path};
use crate::limits::*;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(super) enum Kind {
    Read,
    Write,
    Edit,
    List,
}

pub(super) fn kinds() -> [Kind; 4] {
    [Kind::Read, Kind::Write, Kind::Edit, Kind::List]
}

pub(super) struct FileTool {
    pub workspace: PathBuf,
    pub kind: Kind,
}

#[async_trait]
impl Tool for FileTool {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        let (name, description, effect, properties, required) = match self.kind {
            Kind::Read => (
                "fs_read",
                format!(
                    "Read a UTF-8 file by line range. Relative paths use the workspace; absolute paths \
                     are allowed. offset starts at 1 and limit defaults to {FILE_READ_DEFAULT_LINES}. \
                     Results may end before limit to fit the result budget; the header gives the \
                     offset to continue reading."
                ),
                Effect::ReadOnly,
                json!({
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": FILE_READ_MAX_LINES },
                }),
                vec!["path"],
            ),
            Kind::Write => (
                "fs_write",
                "Atomically replace a UTF-8 file with content, creating parent directories if needed. \
                 Relative paths use the workspace; absolute paths are allowed."
                    .into(),
                Effect::Idempotent,
                json!({
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                }),
                vec!["path", "content"],
            ),
            Kind::Edit => (
                "fs_edit",
                "Replace an exact, unique old_string in a UTF-8 file. Include more context if it occurs \
                 more than once, or use replace_all. The file is written atomically."
                    .into(),
                Effect::SideEffect,
                json!({
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" },
                }),
                vec!["path", "old_string", "new_string"],
            ),
            Kind::List => (
                "fs_list",
                "List one directory, sorted by name, without recursion. Relative paths use the workspace; \
                 absolute paths are allowed."
                    .into(),
                Effect::ReadOnly,
                json!({ "path": { "type": "string" } }),
                vec!["path"],
            ),
        };
        ToolSpec {
            name: name.into(),
            description,
            effect,
            input_schema: closed_object_schema(properties, &required),
        }
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        let result = self.execute(&args, &ctx).await;
        match result {
            Ok(value) => Outcome::Ok { value },
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl FileTool {
    async fn execute(
        &self,
        args: &Map<String, Value>,
        ctx: &CallContext,
    ) -> Result<Value, Failure> {
        let path = resolve_path(&self.workspace, string(args, "path")?);
        match self.kind {
            Kind::Read => read(&path, args, ctx.result_budget).await,
            Kind::Write => {
                let content = string(args, "content")?;
                let path = atomic_write(&path, content, ctx.call).await?;
                Ok(json!({ "path": path, "bytes": content.len() }))
            }
            Kind::Edit => edit(&path, args, ctx.call).await,
            Kind::List => list(&path).await,
        }
    }
}

fn io(path: &Path, error: std::io::Error) -> Failure {
    failure(code::TOOL_FAILED, format!("{}: {error}", path.display()))
}

async fn read(path: &Path, args: &Map<String, Value>, budget: usize) -> Result<Value, Failure> {
    let offset = integer(args, "offset", 1, 1, u64::MAX)?;
    let limit = integer(
        args,
        "limit",
        FILE_READ_DEFAULT_LINES,
        1,
        FILE_READ_MAX_LINES,
    )? as usize;
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| io(path, e))?;
    let total = text.lines().count() as u64;
    let start = usize::try_from(offset - 1).unwrap_or(usize::MAX);
    let mut lines = text.lines().skip(start).take(limit);
    let Some(first) = lines.next() else {
        return Ok(Value::String(format!(
            "{}\n",
            line_header(offset, offset - 1, total)
        )));
    };
    // Always return one whole line; the kernel handles a single oversized line.
    let mut content = first.to_owned();
    let mut header = line_header(offset, offset, total);
    for (last, line) in (offset + 1..).zip(lines) {
        let next_header = line_header(offset, last, total);
        let next_len = next_header.len() + 1 + content.len() + 1 + line.len();
        if next_len > budget {
            break;
        }
        content.push('\n');
        content.push_str(line);
        header = next_header;
    }
    Ok(Value::String(format!("{header}\n{content}")))
}

fn line_header(first: u64, last: u64, total: u64) -> String {
    if last < total {
        format!(
            "[lines {first}-{last} of {total}; continue at offset {}]",
            last + 1
        )
    } else {
        format!("[lines {first}-{last} of {total}]")
    }
}

async fn edit(path: &Path, args: &Map<String, Value>, call: CallId) -> Result<Value, Failure> {
    let old = string(args, "old_string")?;
    let new = string(args, "new_string")?;
    let all = boolean(args, "replace_all", false)?;
    if old.is_empty() || old == new {
        return Err(invalid(
            "old_string must be nonempty and differ from new_string",
        ));
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| io(path, e))?;
    let count = text.matches(old).count();
    if count == 0 {
        return Err(failure(
            code::TOOL_FAILED,
            format!("old_string not found in {}", path.display()),
        ));
    }
    if !all && count != 1 {
        return Err(failure(
            code::TOOL_FAILED,
            format!(
                "old_string occurs {count} times in {}; include more context or set replace_all",
                path.display()
            ),
        ));
    }
    let path = atomic_write(path, &text.replace(old, new), call).await?;
    Ok(json!({ "path": path, "replacements": count }))
}

async fn list(path: &Path) -> Result<Value, Failure> {
    let mut dir = tokio::fs::read_dir(path).await.map_err(|e| io(path, e))?;
    let mut entries = vec![];
    while let Some(entry) = dir.next_entry().await.map_err(|e| io(path, e))? {
        let meta = tokio::fs::symlink_metadata(entry.path())
            .await
            .map_err(|e| io(&entry.path(), e))?;
        let kind = if meta.is_symlink() {
            "symlink"
        } else if meta.is_dir() {
            "dir"
        } else {
            "file"
        };
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "type": kind,
            "size": meta.len(),
        }));
    }
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(json!(entries))
}

async fn atomic_write(path: &Path, content: &str, call: CallId) -> Result<PathBuf, Failure> {
    // Replace the existing target, preserving both symlinks and its permissions.
    let (path, permissions) = match tokio::fs::canonicalize(path).await {
        Ok(target) => {
            let metadata = tokio::fs::metadata(&target)
                .await
                .map_err(|e| io(&target, e))?;
            (target, Some(metadata.permissions()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (path.to_path_buf(), None),
        Err(error) => return Err(io(path, error)),
    };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| io(&path, e))?;
    }
    let temp = path.with_extension(format!("enco-tmp-{call}"));
    let mut file = tokio::fs::File::create(&temp)
        .await
        .map_err(|e| io(&path, e))?;
    let result = async {
        file.write_all(content.as_bytes()).await?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions).await?;
        }
        file.sync_all().await?;
        tokio::fs::rename(&temp, &path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    result.map_err(|e| io(&path, e))?;
    Ok(path)
}
