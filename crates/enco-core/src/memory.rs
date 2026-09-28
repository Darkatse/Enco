use crate::{DateTime, MemoryId, Utc};
use serde::{Deserialize, Serialize};

/// A self-contained fact about the owner; its current authority is the memory database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Identity retained when the fact is corrected, and removed when it is forgotten.
    pub id: MemoryId,
    /// The current statement; recall returns this text rather than indexed copies.
    pub text: String,
    /// Prioritize this fact each Round without retrieval; budget omissions remain explicit.
    pub pinned: bool,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Time of the latest authority update.
    pub updated_at: DateTime<Utc>,
    /// Starts at one and increments on each correction, allowing the index to detect stale content.
    pub rev: u64,
}
