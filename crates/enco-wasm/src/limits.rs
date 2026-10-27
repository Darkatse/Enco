use std::time::Duration;
pub(crate) const EPOCH_TICK: Duration = Duration::from_millis(10);
pub(crate) const WASM_MEMORY_LIMIT: usize = 256 * 1024 * 1024;
pub(crate) const PLUGIN_CALL_TIMEOUT: Duration = Duration::from_secs(330);
pub(crate) const HTTP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const HTTP_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const HTTP_MAX_REDIRECTS: usize = 10;
/// The contract's allowance for service rounding (`decision.answer`).
pub(crate) const DECISION_SUM_TOLERANCE: f64 = 0.01;

/// What every plugin call may use, each as a description and its limit, for the manual.
pub fn call_limits() -> Vec<(String, String)> {
    const MIB: usize = 1024 * 1024;
    [
        (
            "Memory",
            format!(
                "{} MiB; each call starts with fresh memory",
                WASM_MEMORY_LIMIT / MIB
            ),
        ),
        ("Time", format!("{} s", PLUGIN_CALL_TIMEOUT.as_secs())),
        (
            "HTTP timeout when a request sets none",
            format!("{} s", HTTP_DEFAULT_TIMEOUT.as_secs()),
        ),
        (
            "HTTP response body",
            format!("{} MiB", HTTP_MAX_BODY_BYTES / MIB),
        ),
        ("HTTP redirects followed", HTTP_MAX_REDIRECTS.to_string()),
    ]
    .into_iter()
    .map(|(what, limit)| (what.to_owned(), limit))
    .collect()
}

pub(crate) const CHANNEL_RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(60);
