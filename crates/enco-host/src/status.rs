//! Enco's state as `self_status` reports it, and what in it needs attention.
//!
//! Situations and their next steps are typed values derived from the complete state. The
//! application gathers the facts, and `Status::document` writes the text.
mod attention;
mod next;
mod situation;
use crate::{channel::ChannelStatus, development::DevelopmentStatus};
pub use next::{CallStep, FixTarget, OwnerAction, Step, Steps};
pub(crate) use next::{Repair, repair};
use serde::{Serialize, Serializer};
pub use situation::{Attention, AttentionKind, Divergence, GenerationFailure, StopCause};
use std::path::PathBuf;

/// Enco's state now, as `self_status` and `enco status` report it.
///
/// The application assembles this value once for both tools and CLI clients. In JSON, the
/// development facts sit at the top level, beside `node` and the derived `needs_attention`.
pub struct Status {
    /// The three layers of every plugin and of `config.toml`, with recent registrations.
    pub development: DevelopmentStatus,
    /// This host and what it executes.
    pub node: Node,
}

/// This host: its build, its home, and what it executes.
#[derive(Serialize)]
pub struct Node {
    /// Release version of the host, such as `0.1.0`.
    pub version: String,
    /// Git commit the host was built from; a build with uncommitted edits shows the same commit.
    pub revision: String,
    /// The Enco home directory this host runs from.
    pub home: PathBuf,
    /// Execution state from the kernel, such as safe mode, Sessions and the Scheduler; in JSON
    /// its fields sit directly in `node`.
    #[serde(flatten)]
    pub execution: enco_kernel::Status,
    /// Configured channels, such as Telegram, with their health and recent delivery failures.
    pub channels: Vec<ChannelStatus>,
}

impl Status {
    /// Every situation that needs attention now, each with its next steps.
    ///
    /// Situations are derived from the complete value, never cached or appended by callers.
    pub fn attention(&self) -> Vec<Attention<'_>> {
        attention::attention(self)
    }
}

impl Serialize for Status {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            #[serde(flatten)]
            development: &'a DevelopmentStatus,
            node: &'a Node,
            needs_attention: Vec<Attention<'a>>,
        }
        View {
            development: &self.development,
            node: &self.node,
            needs_attention: self.attention(),
        }
        .serialize(serializer)
    }
}
