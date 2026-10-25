use enco_core::{Failure, code};

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
