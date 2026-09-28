use enco_core::{DateTime, Utc};

/// Time boundary used for reminders and observation, never Log ordering.
pub trait Clock: Send + Sync {
    /// Current UTC time.
    fn now(&self) -> DateTime<Utc>;
}
