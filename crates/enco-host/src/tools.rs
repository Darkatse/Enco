//! The host's native tools, and the lifeline: the repair tools that every reply offers unless its
//! profile turns this off.
pub(crate) mod development;
pub(crate) mod failure;
mod files;
mod message;
pub use development::{development_tool_specs, development_tools, self_status_spec};
mod shell;

use enco_core::{Failure, NativeCode, NodeId, SchemaError, WorkingDir};
use enco_kernel::Tool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The repair tools that every reply must offer, unless its profile sets
/// `requires_lifeline = false`.
///
/// They are the minimum the Agent needs to repair Enco, in safe mode and in ordinary operation.
pub const LIFELINE: [&str; 8] = [
    "fs_read",
    "fs_write",
    "fs_edit",
    "fs_list",
    "shell_exec",
    crate::development::STATUS,
    crate::development::APPLY,
    crate::development::REWIND,
];

/// The native tools: the file tools, `message_send` and `shell_exec`, with paths resolved against
/// each Round's working directory.
///
/// They share the working-directory checks and ordinary Tool dispatch.
pub fn native_tools(node: NodeId) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    let mut tools = files::kinds()
        .into_iter()
        .map(|kind| Ok(Arc::new(files::FileTool::new(node, kind)?) as Arc<dyn Tool>))
        .collect::<Result<Vec<_>, SchemaError>>()?;
    tools.push(Arc::new(message::MessageSend::new(node)?));
    tools.push(Arc::new(shell::Shell::new(node)?));
    Ok(tools)
}

/// Resolve a path argument: an absolute path stands alone, a relative one needs the working
/// directory.
pub(crate) async fn resolve_path(
    node: NodeId,
    working_dir: &WorkingDir,
    path: &str,
) -> Result<PathBuf, Failure> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Ok(local_working_dir(node, working_dir).await?.join(path))
}

/// The working directory as a directory on this node; nothing here creates it.
pub(crate) async fn local_working_dir(
    node: NodeId,
    working_dir: &WorkingDir,
) -> Result<&Path, Failure> {
    let unavailable = |reason| failure::working_dir_unavailable(working_dir, reason);
    if working_dir.node != node {
        return Err(unavailable("it is on another node".into()));
    }
    let metadata = tokio::fs::metadata(working_dir.path())
        .await
        .map_err(|error| unavailable(error.to_string()))?;
    if !metadata.is_dir() {
        return Err(unavailable("not a directory".into()));
    }
    Ok(working_dir.path())
}

pub(crate) fn code() -> NativeCode {
    NativeCode {
        name: "host-tools".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
