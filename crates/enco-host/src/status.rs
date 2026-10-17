mod attention;
mod next;
mod situation;
use crate::{channel::ChannelStatus, development::DevelopmentStatus};
pub use next::{CallStep, FixTarget, OwnerAction, Repair, Step, Steps, repair};
use serde::{Serialize, Serializer};
pub use situation::{Attention, AttentionKind, Divergence, GenerationFailure, StopCause};
use std::path::PathBuf;

/// The application assembles this value once for both tools and CLI clients.
pub struct Status {
    pub development: DevelopmentStatus,
    pub node: Node,
}

#[derive(Serialize)]
pub struct Node {
    pub version: String,
    pub revision: String,
    pub home: PathBuf,
    #[serde(flatten)]
    pub execution: enco_kernel::Status,
    pub channels: Vec<ChannelStatus>,
}

impl Status {
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
