use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A model-facing tool contract; its name is unique within a Round snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Name accepted in model tool proposals and matched by dispatch.
    pub name: String,
    /// Instructions explaining the capability to the model.
    pub description: String,
    /// JSON Schema for the tool argument object.
    pub input_schema: Value,
    /// Mutation semantics used when recovering an unsettled call.
    pub effect: Effect,
}

/// Describes external effects so recovery can settle an interrupted invocation honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// No external mutation; interruption can be retried.
    ReadOnly,
    /// Repeating the same operation produces the same intended effect.
    Idempotent,
    /// An external mutation which must not be blindly repeated.
    SideEffect,
}

/// Known success, known failure, or an effect whose outcome is uncertain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// The operation returned a known result.
    Ok {
        /// Result returned by the tool.
        value: Value,
    },
    /// The operation failed with known effects, including cases that never began execution.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
    /// Effects may already have occurred; never retry automatically.
    Unknown {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

/// Durable execution outcome; result text is stored once in the Log entry's content or blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Settlement {
    /// The operation completed successfully.
    Ok,
    /// The operation failed with known effects.
    Failed {
        /// Classification and details of the failure.
        failure: Failure,
    },
    /// Effects may already have occurred; never retry automatically.
    Unknown {
        /// Classification and details of the failure.
        failure: Failure,
    },
}

impl From<&Outcome> for Settlement {
    fn from(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Ok { .. } => Self::Ok,
            Outcome::Failed { failure } => Self::Failed {
                failure: failure.clone(),
            },
            Outcome::Unknown { failure } => Self::Unknown {
                failure: failure.clone(),
            },
        }
    }
}

/// Stable machine-readable classification with contextual diagnostic text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// Stable failure category shared across execution boundaries.
    pub code: String,
    /// Context explaining what failed and how to recover.
    pub message: String,
    /// Whether repeating this request later may succeed.
    pub retryable: bool,
}

/// The single normalization step for raw tool arguments, used by dispatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Arguments {
    /// Normalized object arguments ready for tool validation.
    Object(Map<String, Value>),
    /// Input that cannot be interpreted as an argument object.
    Invalid {
        /// Original invalid input retained for diagnosis.
        raw: String,
    },
}

impl Arguments {
    /// Normalize empty, null, object and once-double-encoded object arguments.
    pub fn parse(raw: &str) -> Self {
        let text = raw.trim();
        if text.is_empty() || text == "null" {
            return Self::Object(Map::new());
        }
        let parsed = serde_json::from_str::<Value>(text);
        let value = match parsed {
            Ok(Value::String(encoded)) => serde_json::from_str(&encoded).ok(),
            Ok(value) => Some(value),
            Err(_) => None,
        };
        match value {
            Some(Value::Object(arguments)) => Self::Object(arguments),
            _ => Self::Invalid { raw: raw.into() },
        }
    }
}

/// Stable failure codes shared with WIT plugins.
pub mod code {
    /// No active generation exports the caller's required interface.
    pub const PLUGIN_UNAVAILABLE: &str = "plugin.unavailable";
    /// An explicit deployment or rollback could not be accepted.
    pub const PLUGIN_REJECTED: &str = "plugin.rejected";
    /// The component trapped or could not be instantiated or invoked.
    pub const PLUGIN_TRAP: &str = "plugin.trap";
    /// The component returned data that violates its WIT interface contract.
    pub const PLUGIN_CONTRACT: &str = "plugin.contract";
    /// The previous process stopped before settlement.
    pub const INTERRUPTED: &str = "interrupted";
    /// A proposed invocation never reached its tool.
    pub const NOT_DISPATCHED: &str = "not_dispatched";
    /// Execution stopped in response to its cancellation token.
    pub const CANCELLED: &str = "cancelled";
    /// The operation exceeded its deadline.
    pub const TIMEOUT: &str = "timeout";
    /// The proposed tool is absent from this Round’s disclosure or snapshot.
    pub const TOOL_UNAVAILABLE: &str = "tool.unavailable";
    /// The tool cannot accept the supplied argument object.
    pub const TOOL_INVALID_ARGUMENTS: &str = "tool.invalid_arguments";
    /// A dispatched tool returned a known failure.
    pub const TOOL_FAILED: &str = "tool.failed";
    /// The composer’s plan violates the execution contract.
    pub const PLAN_INVALID: &str = "plan.invalid";
    /// The composer could not prepare a request.
    pub const COMPOSE_FAILED: &str = "compose.failed";
    /// A context source could not read its authoritative content.
    pub const CONTEXT_FAILED: &str = "context.failed";
    /// The smallest valid request exceeds the configured window.
    pub const CONTEXT_OVERFLOW: &str = "context.overflow";
    /// The Session refers to a profile absent from the current node configuration.
    pub const PROFILE_UNKNOWN: &str = "profile.unknown";
    /// The provider request failed in transport.
    pub const PROVIDER_NETWORK: &str = "provider.network";
    /// The service rejected the configured credentials.
    pub const PROVIDER_AUTH: &str = "provider.auth";
    /// The service asked the caller to slow down.
    pub const PROVIDER_RATE_LIMITED: &str = "provider.rate_limited";
    /// The service returned a server error.
    pub const PROVIDER_SERVER: &str = "provider.server";
    /// The adapter or service rejected the request.
    pub const PROVIDER_BAD_REQUEST: &str = "provider.bad_request";
    /// The service response cannot represent a valid completion or embedding.
    pub const PROVIDER_BAD_RESPONSE: &str = "provider.bad_response";
}

impl ToolSpec {
    /// Reject undeclared names in closed argument objects before dispatch records a start.
    /// This is name validation, not a general JSON Schema validator; tools still validate values.
    pub fn check_argument_names(&self, args: &Map<String, Value>) -> Result<(), Failure> {
        if self.input_schema.get("additionalProperties") != Some(&Value::Bool(false)) {
            return Ok(());
        }
        for name in args.keys() {
            if self
                .input_schema
                .get("properties")
                .and_then(|v| v.get(name))
                .is_none()
            {
                return Err(Failure {
                    code: code::TOOL_INVALID_ARGUMENTS.into(),
                    message: format!(
                        "unexpected argument `{name}` for `{}`; use the declared tool schema",
                        self.name
                    ),
                    retryable: false,
                });
            }
        }
        Ok(())
    }
}

/// Build a closed argument object from property schemas and required property names.
/// Dispatch checks names against this shape; tools validate the values they consume.
pub fn closed_object_schema(properties: Value, required: &[&str]) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}
