use crate::{DateTime, Epoch, EventId, NodeId, ScheduleId, SessionId, Utc};
use serde::{Deserialize, Serialize};

/// Persistent Session identity, execution binding and requirements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Identity joining the Session’s Inbox, Log and execution binding.
    pub id: SessionId,
    /// Unique, human-readable name used by native commands.
    pub name: String,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Node and generation authorized to execute this Session.
    pub binding: Binding,
    /// Requirements declared by this Session.
    pub config: SessionConfig,
}

/// Execution owner and fencing generation; a single node uses itself at epoch one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// Node currently authorized to execute this Session.
    pub node: NodeId,
    /// Ownership generation.
    pub epoch: Epoch,
}

/// Declared requirements enforced when validating a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Whether every reply plan must disclose all lifeline tools.
    pub requires_lifeline: bool,
}

/// A durable one-shot reminder delivered to a Session Inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// Identity used to cancel this reminder and link its delivered Event.
    pub id: ScheduleId,
    /// Destination Session for the reminder Event.
    pub session: SessionId,
    /// Scheduled delivery time in UTC.
    pub due_at: DateTime<Utc>,
    /// Reminder text delivered when due.
    pub message: String,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Current reminder delivery state.
    pub state: ScheduleState,
}

/// Reminder delivery state; firing and Inbox acceptance are atomic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleState {
    /// Not fired or cancelled.
    Pending,
    /// Accepted into the target Inbox.
    Fired {
        /// ID of the reminder Event atomically accepted into the destination Inbox.
        event: EventId,
    },
    /// Cancelled before delivery.
    Cancelled,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            requires_lifeline: true,
        }
    }
}
