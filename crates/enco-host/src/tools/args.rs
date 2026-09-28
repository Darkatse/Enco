use enco_core::{Failure, code};
use serde_json::{Map, Value};
pub(crate) fn failure(code: &str, message: impl Into<String>) -> Failure {
    Failure {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    failure(code::TOOL_INVALID_ARGUMENTS, message)
}

pub(crate) fn string<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, Failure> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{name} must be a string")))
}

pub(crate) fn integer(
    args: &Map<String, Value>,
    name: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, Failure> {
    let value = match args.get(name) {
        None => default,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| invalid(format!("{name} must be an integer")))?,
    };
    if !(min..=max).contains(&value) {
        return Err(invalid(format!("{name} must be between {min} and {max}")));
    }
    Ok(value)
}

pub(crate) fn boolean(
    args: &Map<String, Value>,
    name: &str,
    default: bool,
) -> Result<bool, Failure> {
    match args.get(name) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| invalid(format!("{name} must be a boolean"))),
    }
}
