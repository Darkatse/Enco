pub(crate) mod args;
mod files;
mod shell;

use enco_core::CodeRef;
use enco_kernel::Tool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Minimal repair tools, also required by ordinary Agent Sessions.
pub const LIFELINE: [&str; 5] = ["fs_read", "fs_write", "fs_edit", "fs_list", "shell_exec"];

/// Native capabilities share the workspace path rules and ordinary Tool dispatch.
pub fn native_tools(workspace: PathBuf) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = files::kinds()
        .into_iter()
        .map(|kind| {
            Arc::new(files::FileTool {
                workspace: workspace.clone(),
                kind,
            }) as Arc<dyn Tool>
        })
        .collect();
    tools.push(Arc::new(shell::Shell { workspace }));
    tools
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
