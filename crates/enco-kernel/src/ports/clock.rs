use enco_core::{DateTime, Utc};

/// Time boundary used for reminders and observation, never Log ordering.
pub trait Clock: Send + Sync {
    /// Current UTC instant; local time is derived using the owner's configured zone.
    fn now(&self) -> DateTime<Utc>;
}
