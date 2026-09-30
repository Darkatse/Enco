use chrono::{Local, SubsecRound};
use enco_core::{DateTime, FixedOffset};
use enco_kernel::Clock;

/// Host wall clock; Log order is assigned independently by the Session.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset().trunc_subsecs(3)
    }
}
