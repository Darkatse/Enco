use crate::{DateTime, LogPos, SessionId, Settlement, Utc};
use serde::{Deserialize, Serialize};

/// A logical channel delivery referencing its original Log fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    /// Session containing the original reply or terminal notice.
    pub session: SessionId,
    /// RoundEnded or RunEnded position which caused this delivery.
    pub pos: LogPos,
    /// Protocol-local destination.
    pub target: String,
}

/// Final outcome, committed atomically with the connection's outbound progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliverySettlement {
    /// Original Log reference and destination.
    pub delivery: Delivery,
    /// One final outcome for the entire logical delivery.
    pub outcome: Settlement,
    /// Observation time for display, never ordering.
    pub at: DateTime<Utc>,
}
