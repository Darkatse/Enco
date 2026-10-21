use crate::{CallId, Image};
use serde::{Deserialize, Serialize};

/// The speaker in a canonical model message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Instructions supplied by the composer.
    System,
    /// Owner input and contextual messages from event producers or the composer.
    User,
    /// Model output, including proposed calls.
    Assistant,
    /// Settlement of a proposed call.
    Tool,
}

/// Provider-independent message; this shape is part of the Log format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message<I = Image> {
    /// Speaker of this message.
    pub role: Role,
    /// Content in provider order.
    pub parts: Vec<Part<I>>,
}

/// Typed content of a canonical message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part<I = Image> {
    /// UTF-8 text visible to the model.
    Text {
        /// Text visible to the model as part of this message.
        text: String,
    },
    /// A proposed invocation; arguments are normalized only when dispatched.
    ToolCall(ToolCall),
    /// Model-facing settlement linked to the original provider call ID.
    ToolResult(ToolResult),
    /// Adapter-specific fields, such as reasoning, replayed only by their originating Provider.
    Extension(Extension),
    /// Image input, represented by a recorded reference or resolved request bytes.
    Image(I),
}

/// A proposed invocation; arguments are normalized only when dispatched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Fresh identity assigned by the host adapter when accepting a Completion.
    pub id: CallId,
    /// Original service call ID, empty when the service assigned none.
    pub provider_id: String,
    /// Tool name selected by the model from the disclosed definitions.
    pub name: String,
    /// Raw provider arguments, parsed only at dispatch.
    pub arguments: String,
}

/// Model-facing settlement linked to the original provider call ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Host identity of the proposed tool call.
    pub call: CallId,
    /// Original service call ID, retained when returning tool results.
    pub provider_id: String,
    /// Exact model-visible result text.
    pub content: String,
    /// Whether this is a failed or unknown tool outcome.
    pub is_error: bool,
}

/// Opaque provider-owned message fields retained for subsequent requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extension {
    /// Opaque JSON; the producing Attempt's generation identifies its owner.
    pub data: serde_json::Value,
}

impl<I> Part<I> {
    /// The same part under another image type; an image is handed back for the caller to convert.
    pub fn retype<J>(self) -> Result<Part<J>, I> {
        match self {
            Self::Text { text } => Ok(Part::Text { text }),
            Self::ToolCall(call) => Ok(Part::ToolCall(call)),
            Self::ToolResult(result) => Ok(Part::ToolResult(result)),
            Self::Extension(extension) => Ok(Part::Extension(extension)),
            Self::Image(image) => Err(image),
        }
    }
}

impl<I> Message<I> {
    /// Construct a text-only message.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            parts: vec![Part::Text { text: text.into() }],
        }
    }

    /// Proposed calls in provider order.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.parts.iter().filter_map(|part| match part {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    /// Join visible text, excluding provider extensions and tool calls.
    pub fn joined_text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
