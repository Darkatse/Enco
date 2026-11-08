//! Session execution through explicit ports.
#![warn(missing_docs)]

mod attempt;
mod builtin;
mod child_slots;
mod dispatch;
mod image;
mod inspect;
mod kernel;
mod limits;
mod outgoing;
mod plan;
mod ports;
mod profile;
mod reach;
mod recovery;
mod registry;
mod round;
mod run;
mod scheduler;
mod session;
mod settings;
mod snapshot;
mod subtree;
mod transcript;

pub use builtin::builtin_tool_specs;
pub use inspect::Inspection;
pub use kernel::*;
pub use limits::{CHANNEL_MAX_BACKOFF, IMAGE_MAX_BYTES, TRIAL_CALLS};
pub use ports::*;
pub use profile::*;
pub use registry::*;
pub use scheduler::{ScheduleError, Scheduled, Schedules};
pub use settings::*;

#[cfg(test)]
mod test_support;
