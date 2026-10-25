pub(crate) mod development;
pub(crate) mod failure;
mod files;
mod message;
pub use development::{development_tool_specs, development_tools, self_status_spec};
mod shell;

use enco_core::{CodeRef, SchemaError};
use enco_kernel::Tool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Minimal repair tools, also required by ordinary Agent Sessions.
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

/// Native capabilities share the workspace path rules and ordinary Tool dispatch.
pub fn native_tools(workspace: PathBuf) -> Result<Vec<Arc<dyn Tool>>, SchemaError> {
    let mut tools = files::kinds()
        .into_iter()
        .map(|kind| Ok(Arc::new(files::FileTool::new(workspace.clone(), kind)?) as Arc<dyn Tool>))
        .collect::<Result<Vec<_>, SchemaError>>()?;
    tools.push(Arc::new(message::MessageSend::new(workspace.clone())?));
    tools.push(Arc::new(shell::Shell::new(workspace)?));
    Ok(tools)
}

pub(crate) fn resolve_path(workspace: &Path, path: &str) -> PathBuf {
    workspace.join(path)
}

pub(crate) fn code() -> CodeRef {
    CodeRef::Native {
        name: "host-tools".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}
