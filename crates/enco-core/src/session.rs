use crate::{DateTime, Epoch, EventId, NodeId, ScheduleId, SessionId, Tz, Utc};
use serde::{Deserialize, Serialize};

/// Profile selected when a Session is created; every node configuration must define it.
pub const DEFAULT_PROFILE: &str = "default";

/// Persistent Session identity, execution binding and selected profile.
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
    /// Name of the configuration selected at the next Round boundary.
    pub profile: String,
}

/// Execution owner and fencing generation; a single node uses itself at epoch one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// Node currently authorized to execute this Session.
    pub node: NodeId,
    /// Ownership generation.
    pub epoch: Epoch,
}

/// A durable sequence of reminders delivered to a Session Inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// Identity used to cancel this schedule and link its delivered Events.
    pub id: ScheduleId,
    /// Destination Session for the reminder Event.
    pub session: SessionId,
    /// Rule from which future occurrences are derived.
    pub rule: ScheduleRule,
    /// Reminder text delivered when due.
    pub message: String,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Current schedule lifetime state.
    pub state: ScheduleState,
    /// Most recent occurrence atomically accepted into the Inbox.
    pub last: Option<Occurrence>,
}

/// One instant or a recurring pattern interpreted in local time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleRule {
    /// A single future instant.
    Once {
        /// Planned delivery time in UTC.
        at: DateTime<Utc>,
    },
    /// A five-field cron expression (minute, hour, day, month, weekday).
    Cron {
        /// Pattern as provided by the caller.
        expr: String,
        /// IANA time zone; absent means follow the owner's configured zone.
        timezone: Option<Tz>,
    },
}

/// Last delivered occurrence; the next one is derived rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    /// Planned time, independent of when the scheduler actually delivered it.
    pub due_at: DateTime<Utc>,
    /// Event accepted into the destination Inbox.
    pub event: EventId,
}

/// Schedule lifetime; firing and Inbox acceptance are atomic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleState {
    /// Future occurrences may be delivered.
    Active,
    /// The last occurrence has been delivered.
    Done,
    /// Future occurrences have been cancelled.
    Cancelled,
}
