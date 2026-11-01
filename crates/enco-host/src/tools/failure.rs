//! The failures that native tools return.
use enco_core::{Failure, WorkingDir, code};

pub(crate) fn failure(code: &str, message: impl Into<String>) -> Failure {
    Failure {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

/// Arguments that fit the schema but break a rule it cannot express, such as two that conflict.
pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    failure(code::TOOL_INVALID_ARGUMENTS, message)
}

/// The call needs the working directory, which cannot be used here; the call did not run.
pub(crate) fn working_dir_unavailable(working_dir: &WorkingDir, reason: String) -> Failure {
    failure(
        code::TOOL_FAILED,
        format!(
            "Working directory {} is unavailable: {reason}. Use absolute paths instead, including an absolute cwd for shell_exec, or ask the owner to change it with `enco working-dir`.",
            working_dir.path().display()
        ),
    )
}
