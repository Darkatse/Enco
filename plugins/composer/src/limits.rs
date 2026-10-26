//! Budgeting policy for the factory composer.
pub(super) const COMPACTION_TRIGGER_PERCENT: u32 = 80;
pub(super) const COMPACTION_TAIL_PERCENT: u32 = 30;
pub(super) const INSTRUCTION_PERCENT: u32 = 10;
pub(super) const MEMORY_PERCENT: u32 = 15;
pub(super) const COMPACTION_TOOL_BYTES: usize = 2_000;
pub(super) const REQUEST_IMAGE_MAX_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const IMAGE_TOKENS: u32 = 1600;
/// Request bytes around one message, and around each of its parts, besides their text.
pub(super) const MESSAGE_OVERHEAD_BYTES: usize = 32;
pub(super) const PART_OVERHEAD_BYTES: usize = 48;
