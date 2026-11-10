use enco_core::*;
use std::sync::Arc;

/// An atomic append of Log facts, Inbox consumption and cross-Session inputs.
pub struct Commit {
    /// Ordered facts to append.
    pub entries: Vec<Entry>,
    /// A request plan accepted with the Attempt that refers to it.
    pub plan: Option<Arc<super::PlanWrite>>,
    /// Inputs committed with these facts, in order.
    pub deliveries: Vec<SessionDelivery>,
}

/// One input handed from a Session owner to another Session's Inbox.
pub struct SessionDelivery {
    /// Sender-assigned identity and complete input.
    pub event: Event,
    /// Preconditions and whether failed delivery may leave the local facts committed.
    pub kind: SessionDeliveryKind,
}

/// Meaning of a cross-Session write within its owning Session's commit.
pub enum SessionDeliveryKind {
    /// A Round's reply or a Run's end; when a halt or archival blocks it, the facts still commit.
    Boundary,
    /// A message's effect is delivery itself; rejection leaves the entire commit unwritten.
    Message,
    /// A delegation's effect, with optional creation and assertions about an existing child.
    Delegation {
        /// Target child's full name.
        name: SessionName,
        /// The new child's creation record, also in the parent's Log; absent when addressing an
        /// existing child.
        create: Option<Box<Creation>>,
        /// Supplied profile must match an existing child's effective profile.
        profile: Option<ProfileName>,
        /// Supplied directory must match an existing child's effective directory.
        working_dir: Option<WorkingDir>,
    },
}

/// Result of checking and committing a Session owner's transaction.
pub enum CommitOutcome {
    /// Facts committed, with the actual Inbox recipients to wake after the transaction.
    Committed {
        /// Sessions whose Inbox accepted a new input.
        delivered: Vec<SessionId>,
    },
    /// No changes were written; the owning actor must record the tool's failure instead.
    Rejected(CommitRejection),
}

/// A normal tool precondition failure, distinct from a storage failure.
#[derive(Debug, thiserror::Error)]
pub enum CommitRejection {
    /// The sending Session has an unconsumed halt.
    #[error("this Session has been halted; no message was sent")]
    Halted,
    /// The recipient no longer accepts inputs.
    #[error("session `{0}` is archived and accepts no more messages")]
    Archived(SessionName),
    /// Creating a new child requires a profile.
    #[error(
        "child Session `{}` does not exist; pass `profile` to create it",
        .0.short_name()
    )]
    MissingProfile(SessionName),
    /// Another creation occupied this name before commit.
    #[error(
        "child Session `{}` already exists; omit `profile` and `working_dir` to send to it",
        .0.short_name()
    )]
    ChildExists(SessionName),
    /// The supplied profile disagrees with the existing child's profile.
    #[error(
        "child Session uses profile `{current}`, not `{requested}`; omit `profile` to send to it"
    )]
    ProfileMismatch {
        /// Existing profile.
        current: ProfileName,
        /// Supplied profile.
        requested: ProfileName,
    },
    /// Only the owner may change an existing child's working directory.
    #[error(
        "child Session uses working directory `{}`, not `{}`; omit `working_dir` to send to it",
        .current.path_str(),
        .requested.path_str()
    )]
    WorkingDirMismatch {
        /// Existing directory.
        current: WorkingDir,
        /// Supplied directory.
        requested: WorkingDir,
    },
    /// The running configuration has no profile by this name for a new child.
    #[error("unknown profile `{0}`; create the child with one of the configured profiles")]
    ProfileUnknown(ProfileName),
}
