use chrono::SubsecRound;
use enco_core::{DateTime, Utc};
use enco_kernel::Clock;

/// Host wall clock; Log order is assigned independently by the Session.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now().trunc_subsecs(3)
    }
}
