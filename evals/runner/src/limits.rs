use std::time::Duration;

pub(crate) const DEFAULT_K: usize = 5;
pub(crate) const ALPHA: f64 = 0.05;
pub(crate) const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const CLI_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
pub(crate) const GRADER_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const JUDGE_RETRIES: usize = 2;
pub(crate) const REASON_BYTES: usize = 2000;
pub(crate) const NULL_EMBEDDING_DIMENSIONS: usize = 8;
pub(crate) const STATUS_INTERVAL: Duration = Duration::from_millis(100);
