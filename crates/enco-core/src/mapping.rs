//! Immutable evidence for pure calls that translate external protocol responses.
use crate::{CodeRef, ContentHash};
use serde::{Deserialize, Serialize};

/// A recorded mapper call; each response body is separate evidence that its arguments address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingRecord {
    /// The mapper code that interpreted these arguments: a generation or factory code.
    pub code: CodeRef,
    /// The exported function, such as `channel.received`.
    pub export: String,
    /// Complete arguments, with each response body replaced by its content address.
    pub arguments: serde_json::Value,
}

/// An output's position in one immutable mapper call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingOrigin {
    /// Evidence address of the serialized mapping record.
    pub call: ContentHash,
    /// Zero-based position in the returned values; zero for a single result.
    pub index: u32,
}
