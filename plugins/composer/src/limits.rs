//! Budgeting policy for the factory composer.
pub(super) const COMPACTION_TRIGGER_PERCENT: u32 = 80;
pub(super) const INSTRUCTION_PERCENT: u32 = 10;
pub(super) const MEMORY_PERCENT: u32 = 15;
pub(super) const COMPACTION_TOOL_BYTES: usize = 2_000;
pub(super) const REQUEST_IMAGE_MAX_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const IMAGE_TOKENS: u32 = 1600;
/// Request bytes around one message, and around each of its parts, besides their text.
pub(super) const MESSAGE_OVERHEAD_BYTES: usize = 32;
pub(super) const PART_OVERHEAD_BYTES: usize = 48;
/// After a refusal with the service's count, the share of the accepted input a new request
/// aims for, leaving room for the estimate to err differently on what is cut and what is kept.
pub(super) const OVERFLOW_MARGIN_PERCENT: u64 = 90;
/// The share of a refused request the next one aims for when the service gave no count.
pub(super) const OVERFLOW_DEFAULT_PERCENT: u64 = 50;
