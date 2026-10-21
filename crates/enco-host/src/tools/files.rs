use super::{args::*, resolve_path};
use crate::limits::*;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub(super) enum Kind {
    Read,
    Write,
    Edit,
    List,
}

pub(super) fn kinds() -> [Kind; 4] {
    [Kind::Read, Kind::Write, Kind::Edit, Kind::List]
}

/// Local file IO finishes quickly, so file tools ignore cancellation and settle normally;
/// a blocking special file such as a FIFO holds cancellation and shutdown until it returns.
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
                    "Read a PNG, JPEG or WebP image, or a UTF-8 file by line range. \
                     Relative paths use the workspace; absolute paths are allowed. For text, offset \
                     starts at 1 and limit defaults to {FILE_READ_DEFAULT_LINES}. \
                     Results may end before limit to fit the result budget; the header gives the \
                     offset to continue reading. A single line longer than the budget comes back \
                     truncated; read it by byte range in the shell, for example with tail -c and head -c."
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
            Ok(outcome) => outcome,
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl FileTool {
    async fn execute(
        &self,
        args: &Map<String, Value>,
        ctx: &CallContext,
    ) -> Result<Outcome, Failure> {
        let path = resolve_path(&self.workspace, string(args, "path")?);
        let value = match self.kind {
            Kind::Read => return read(&path, args, ctx.result_budget).await,
            Kind::Write => {
                let content = string(args, "content")?;
                let path = crate::file::write(&path, content.as_bytes())
                    .await
                    .map_err(|error| io(&path, error))?;
                Ok(json!({ "path": path, "bytes": content.len() }))
            }
            Kind::Edit => edit(&path, args).await,
            Kind::List => list(&path).await,
        }?;
        Ok(Outcome::Ok {
            value,
            images: Vec::new(),
        })
    }
}

fn io(path: &Path, error: std::io::Error) -> Failure {
    failure(code::TOOL_FAILED, format!("{}: {error}", path.display()))
}

async fn read(path: &Path, args: &Map<String, Value>, budget: usize) -> Result<Outcome, Failure> {
    let offset = integer(args, "offset", 1, 1, u64::MAX)?;
    let limit = integer(
        args,
        "limit",
        FILE_READ_DEFAULT_LINES,
        1,
        FILE_READ_MAX_LINES,
    )? as usize;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| io(path, error))?;
    if ImageFormat::detect(&bytes).is_some() {
        return Ok(Outcome::Ok {
            value: json!({ "path": path }),
            images: vec![bytes],
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|error| {
        failure(
            code::TOOL_FAILED,
            format!(
                "{}: expected UTF-8 text or a PNG, JPEG or WebP image: {error}",
                path.display()
            ),
        )
    })?;
    Ok(Outcome::Ok {
        value: read_text(text, offset, limit, budget),
        images: Vec::new(),
    })
}

fn read_text(text: &str, offset: u64, limit: usize, budget: usize) -> Value {
    let total = text.lines().count() as u64;
    let start = usize::try_from(offset - 1).unwrap_or(usize::MAX);
    let mut lines = text.lines().skip(start).take(limit);
    let Some(first) = lines.next() else {
        return Value::String(format!("{}\n", line_header(offset, offset - 1, total)));
    };
    // Always return one whole line; the kernel truncates a single oversized line.
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
    Value::String(format!("{header}\n{content}"))
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

async fn edit(path: &Path, args: &Map<String, Value>) -> Result<Value, Failure> {
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
    let path = crate::file::write(path, text.replace(old, new).as_bytes())
        .await
        .map_err(|error| io(path, error))?;
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
