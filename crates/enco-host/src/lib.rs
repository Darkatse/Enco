//! Native storage, tools and context policies.

pub mod channel;
mod clock;
mod composer;
mod context;
pub mod development;
mod file;
mod limits;
pub mod memory;
mod sqlite;
pub mod status;
mod store;
pub mod telegram;
mod tools;

pub use clock::SystemClock;
pub use composer::FactoryComposer;
pub use context::InstructionsContextSource;
pub use memory::{
    Memories, MemoryContextSource, MemoryError, MemoryList, MemoryPaths, Recall, memory_tool_specs,
    memory_tools,
};
pub use store::{SqliteStore, StorePaths};
pub use telegram::Telegram;
pub use tools::{
    LIFELINE, development_tool_specs, development_tools, native_tools, self_status_spec,
};

/// Configuration ownership for the host components assembled by the application.
pub const TABLE_OWNERS: [enco_kernel::TableOwner; 2] = [memory::TABLE_OWNER, telegram::TABLE_OWNER];
