use std::time::Duration;
pub(crate) const MAX_ATTEMPTS: u32 = 3;
pub(crate) const BACKOFF: [Duration; MAX_ATTEMPTS as usize - 1] =
    [Duration::from_secs(1), Duration::from_secs(4)];
pub(crate) const MAX_COMPACTIONS_PER_ROUND: u32 = 2;
pub(crate) const TOOL_RESULT_INLINE_BYTES: usize = 16 * 1024;
pub(crate) const TOOL_RESULT_PREVIEW_BYTES: usize = 4 * 1024;
pub(crate) const SESSION_BROADCAST_CAPACITY: usize = 256;
pub(crate) const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub(crate) const SCHEDULER_CHANNEL_CAPACITY: usize = 64;
pub(crate) const MIN_RECURRENCE_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub(crate) const RECURRENCE_CHECK: usize = 100;

/// Successful trial calls required before a generation becomes a rollback target.
pub const TRIAL_CALLS: u32 = 5;
