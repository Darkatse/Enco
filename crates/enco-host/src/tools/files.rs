use super::{failure::*, resolve_path};
use crate::limits::*;
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{CallContext, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    num::NonZeroU64,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Read,
    Write,
    Edit,
    List,
}

pub(super) fn kinds() -> [Kind; 4] {
    [Kind::Read, Kind::Write, Kind::Edit, Kind::List]
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    /// The file to read, relative to the workspace or absolute.
    path: String,
    /// The first line to return, counting from 1.
    #[serde(default = "first_line")]
    offset: NonZeroU64,
    /// The number of lines to return.
    #[serde(default = "default_lines")]
    #[schemars(range(min = 1, max = FILE_READ_MAX_LINES))]
    limit: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteArguments {
    /// The file to write, relative to the workspace or absolute.
    path: String,
    /// The complete new text of the file.
    content: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditArguments {
    /// The file to edit, relative to the workspace or absolute.
    path: String,
    /// The exact text to replace.
    #[schemars(length(min = 1))]
    old_string: String,
    /// The replacement text; it must differ from `old_string`.
    new_string: String,
    /// Whether to replace every occurrence instead of exactly one.
    #[serde(default)]
    replace_all: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArguments {
    /// The directory to list, relative to the workspace or absolute.
    path: String,
}

fn first_line() -> NonZeroU64 {
    NonZeroU64::MIN
}

fn default_lines() -> u64 {
    FILE_READ_DEFAULT_LINES
}

impl Kind {
    fn spec(self) -> Result<ToolSpec, SchemaError> {
        match self {
            Kind::Read => ToolSpec::of::<ReadArguments>(
                "fs_read",
                "Read a PNG, JPEG or WebP image whole, or a UTF-8 file by line range. \
                 Results may end before limit to fit the result budget; the header gives the \
                 offset to continue reading. A single line longer than the budget comes back \
                 truncated; read it by byte range in the shell, for example with tail -c and head -c.",
                Effect::ReadOnly,
            ),
            Kind::Write => ToolSpec::of::<WriteArguments>(
                "fs_write",
                "Atomically replace a UTF-8 file with content, creating parent directories if needed.",
                Effect::Idempotent,
            ),
            Kind::Edit => ToolSpec::of::<EditArguments>(
                "fs_edit",
                "Replace an exact, unique old_string in a UTF-8 file. Include more context if it occurs \
                 more than once, or use replace_all. The file is written atomically.",
                Effect::SideEffect,
            ),
            Kind::List => ToolSpec::of::<ListArguments>(
                "fs_list",
                "List one directory, sorted by name, without recursion.",
                Effect::ReadOnly,
            ),
        }
    }
}

/// A host tool that reads, writes, edits or lists local files.
///
/// Local file IO finishes quickly, so file tools ignore cancellation and settle normally;
/// a blocking special file such as a FIFO holds cancellation and shutdown until it returns.
pub(super) struct FileTool {
    workspace: PathBuf,
    kind: Kind,
    spec: ToolSpec,
}

impl FileTool {
    pub(super) fn new(workspace: PathBuf, kind: Kind) -> Result<Self, SchemaError> {
        Ok(Self {
            workspace,
            spec: kind.spec()?,
            kind,
        })
    }
}

#[async_trait]
impl Tool for FileTool {
    fn code(&self) -> CodeRef {
        super::code()
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, ctx: CallContext, args: Map<String, Value>) -> Outcome {
        let result = self.execute(args, &ctx).await;
        match result {
            Ok(outcome) => outcome,
            Err(failure) => Outcome::Failed { failure },
        }
    }
}

impl FileTool {
    async fn execute(
        &self,
        args: Map<String, Value>,
        ctx: &CallContext,
    ) -> Result<Outcome, Failure> {
        let value = match self.kind {
            Kind::Read => {
                let arguments: ReadArguments = parse_arguments(args)?;
                return read(
                    &self.resolve(&arguments.path),
                    &arguments,
                    ctx.result_budget,
                )
                .await;
            }
            Kind::Write => {
                let WriteArguments { path, content } = parse_arguments(args)?;
                let path = self.resolve(&path);
                let path = crate::file::write(&path, content.as_bytes())
                    .await
                    .map_err(|error| io(&path, error))?;
                Ok(json!({ "path": path, "bytes": content.len() }))
            }
            Kind::Edit => {
                let arguments: EditArguments = parse_arguments(args)?;
                edit(&self.resolve(&arguments.path), &arguments).await
            }
            Kind::List => {
                let ListArguments { path } = parse_arguments(args)?;
                list(&self.resolve(&path)).await
            }
        }?;
        Ok(Outcome::Ok {
            value,
            images: Vec::new(),
            outgoing: None,
        })
    }

    fn resolve(&self, path: &str) -> PathBuf {
        resolve_path(&self.workspace, path)
    }
}

fn io(path: &Path, error: std::io::Error) -> Failure {
    failure(code::TOOL_FAILED, format!("{}: {error}", path.display()))
}

async fn read(path: &Path, arguments: &ReadArguments, budget: usize) -> Result<Outcome, Failure> {
    let (offset, limit) = (arguments.offset.get(), arguments.limit as usize);
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| io(path, error))?;
    if ImageFormat::detect(&bytes).is_some() {
        return Ok(Outcome::Ok {
            value: json!({ "path": path }),
            images: vec![bytes],
            outgoing: None,
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
        outgoing: None,
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

async fn edit(path: &Path, arguments: &EditArguments) -> Result<Value, Failure> {
    let (old, new, all) = (
        arguments.old_string.as_str(),
        arguments.new_string.as_str(),
        arguments.replace_all,
    );
    if old == new {
        return Err(invalid("new_string must differ from old_string"));
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
