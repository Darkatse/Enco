use enco_core::{DateTime, FixedOffset};

/// Time boundary used for reminders and observation, never Log ordering.
pub trait Clock: Send + Sync {
    /// Current time with the host's current UTC offset; durable timestamps use to_utc().
    fn now(&self) -> DateTime<FixedOffset>;
}
