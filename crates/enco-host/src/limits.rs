//! Fixed P0 policy limits; these are not runtime configuration.
pub(crate) const FILE_READ_DEFAULT_LINES: u64 = 2_000;
pub(crate) const FILE_READ_MAX_LINES: u64 = 10_000;
// Telegram states its upload limits in MB; decimal bytes stay within either reading.
pub(crate) const MESSAGE_FILE_MAX_BYTES: u64 = 50_000_000;
pub(crate) const SHELL_DEFAULT_TIMEOUT_MS: u64 = 120_000;
pub(crate) const SHELL_MAX_TIMEOUT_MS: u64 = 1_800_000;
pub(crate) const SHELL_OUTPUT_BYTES: usize = 1024 * 1024;
pub(crate) const SHELL_EXIT_GRACE: std::time::Duration = std::time::Duration::from_millis(500);
pub(crate) const MEMORY_TEXT_BYTES: usize = 2_000;
pub(crate) const MEMORY_RECALL_DEFAULT: u64 = 8;
pub(crate) const MEMORY_RECALL_MAX: u64 = 20;
pub(crate) const MEMORY_RELEVANCE_THRESHOLD: f64 = 0.5;
pub(crate) const EMBED_BATCH: usize = 64;

// A destination that stays unavailable must not starve later logical deliveries.
pub(crate) const CHANNEL_SEND_ATTEMPTS: u32 = 4;
pub(crate) const CHANNEL_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
pub(crate) const CHANNEL_WAKE_CAPACITY: usize = 32;
// Renewed before the shortest protocol indicator lapses: Telegram's lasts five seconds.
pub(crate) const CHANNEL_TYPING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(4);
pub(crate) const RECENT_DELIVERY_FAILURES: u32 = 10;
pub(crate) const BUILD_DIAGNOSTICS: usize = 12;
pub(crate) const PLAN_CHAIN_LIMIT: usize = 32;
