//! Bot API limits and transport deadlines used by the mapper.
pub(super) const POLL_SECONDS: u32 = 30;
pub(super) const POLL_TIMEOUT_MS: u32 = (POLL_SECONDS + 10) * 1000;
pub(super) const READ_TIMEOUT_MS: u32 = 30_000;
pub(super) const SEND_TIMEOUT_MS: u32 = 15_000;
pub(super) const UPLOAD_TIMEOUT_MS: u32 = 120_000;
pub(super) const MESSAGE_CHARS: usize = 32768;
// Telegram states these limits in MB; decimal bytes stay within either reading.
pub(super) const PHOTO_MAX_BYTES: usize = 10_000_000;
pub(super) const PHOTO_MAX_DIMENSION_SUM: u64 = 10_000;
pub(super) const PHOTO_MAX_ASPECT_RATIO: u64 = 20;
