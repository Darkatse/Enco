//! The host's wall clock, and the format of the local times that the model and the owner read.
use chrono::SubsecRound;
use enco_core::{DateTime, Tz, Utc};
use enco_kernel::Clock;

/// Host wall clock; Log order is assigned independently by the Session.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now().trunc_subsecs(3)
    }
}

/// `at` as the owner reads it in model context, operational documents and receipts, such as
/// `2026-11-03 09:30, Tuesday`.
pub(crate) fn local_time(at: DateTime<Utc>, timezone: Tz) -> String {
    at.with_timezone(&timezone)
        .format("%Y-%m-%d %H:%M, %A")
        .to_string()
}
