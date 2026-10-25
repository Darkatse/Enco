//! The host's wall clock, and the format of the local times that the model and the owner read.
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

/// Owner-local time shown in model context and operational documents.
pub(crate) const LOCAL_TIME: &str = "%Y-%m-%d %H:%M, %A";
