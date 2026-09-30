//! Durable vocabulary shared by the kernel, host and local protocol.
#![warn(missing_docs)]
mod capability;
mod delivery;
mod entry;
mod event;
mod hash;
mod ids;
mod memory;
mod message;
mod plan;
mod session;
mod tool;
pub use capability::*;
pub use chrono::{DateTime, FixedOffset, Utc};
pub use delivery::*;
pub use entry::*;
pub use event::*;
pub use hash::*;
pub use ids::*;
pub use memory::*;
pub use message::*;
pub use plan::*;
pub use session::*;
pub use tool::*;

/// Conservative shared estimate; the provider's Usage remains authoritative.
pub fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.len().div_ceil(3)).unwrap_or(u32::MAX)
}
