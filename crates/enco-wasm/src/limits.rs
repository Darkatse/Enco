use std::time::Duration;
pub(crate) const EPOCH_TICK: Duration = Duration::from_millis(10);
pub(crate) const WASM_MEMORY_LIMIT: usize = 256 * 1024 * 1024;
pub(crate) const PLUGIN_CALL_TIMEOUT: Duration = Duration::from_secs(330);
pub(crate) const HTTP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const HTTP_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
/// The contract's allowance for service rounding (`decision.answer`).
pub(crate) const DECISION_SUM_TOLERANCE: f64 = 0.01;
