//! Immutable evidence for pure calls that translate external protocol responses.
use crate::{ContentHash, GenerationId};
use serde::{Deserialize, Serialize};

/// A recorded mapper call; response bodies are separate blobs referenced by its arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingRecord {
    /// The registered generation that interpreted these arguments.
    pub generation: GenerationId,
    /// The exported function, such as `channel.received`.
    pub export: String,
    /// Complete arguments, with each response body replaced by its content address.
    pub arguments: serde_json::Value,
}

/// An output's position in one immutable mapper call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingOrigin {
    /// Content address of the serialized mapping record.
    pub call: ContentHash,
    /// Zero-based position in the returned values; zero for a single result.
    pub index: u32,
}
