use crate::{DateTime, Epoch, NodeId, SessionId, Utc};
use serde::{Deserialize, Serialize};

/// Profile selected when a Session is created; every node configuration must define it.
pub const DEFAULT_PROFILE: &str = "default";

/// A Session's stable identity with its resolved name for inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    /// Identity used by the Session's durable records.
    pub id: SessionId,
    /// Human-readable name resolved by the record owner.
    pub name: crate::SessionName,
}

/// Persistent Session identity, execution binding, selected profile and working directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Identity joining the Session’s Inbox, Log and execution binding.
    pub id: SessionId,
    /// Human-readable full name, unique among unarchived Sessions.
    pub name: crate::SessionName,
    /// Parent fixed at creation; absent for a root Session.
    pub parent: Option<SessionRef>,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// Time this Session stopped accepting inputs; absent while unarchived.
    pub archived_at: Option<DateTime<Utc>>,
    /// Node and generation authorized to execute this Session.
    pub binding: Binding,
    /// Name of the configuration selected at the next Round boundary.
    pub profile: crate::ProfileName,
    /// Working directory sampled at the next Round boundary.
    pub working_dir: crate::WorkingDir,
}

/// What the authority over a Session's name records when it creates the Session: its identity
/// and where it starts.
///
/// A root's authority is the root directory; a child's is its parent's Log. Later changes are the
/// Session's own Log facts, and its archive is a control-plane fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Creation {
    /// Identity joining the Session's Inbox, Log and execution binding.
    pub id: SessionId,
    /// Full name, unique among unarchived Sessions when it was created.
    pub name: crate::SessionName,
    /// The parent, absent for a root.
    pub parent: Option<SessionRef>,
    /// Creation time in UTC.
    pub created_at: DateTime<Utc>,
    /// The node that first executes it.
    pub node: NodeId,
    /// The profile it starts with.
    pub profile: crate::ProfileName,
    /// The working directory it starts with.
    pub working_dir: crate::WorkingDir,
}

impl SessionRecord {
    /// The row a creation record starts: unarchived, executed by its first node at epoch one.
    pub fn created(creation: &Creation) -> Self {
        Self {
            id: creation.id,
            name: creation.name.clone(),
            parent: creation.parent.clone(),
            created_at: creation.created_at,
            archived_at: None,
            binding: Binding {
                node: creation.node,
                epoch: Epoch(1),
            },
            profile: creation.profile.clone(),
            working_dir: creation.working_dir.clone(),
        }
    }

    /// Apply one input the Session consumed, or will consume: only a command acting on the
    /// Session changes its row.
    ///
    /// The same function folds the Log into the committed view and the pending Inbox into the
    /// effective view, so the two views cannot disagree about what a command does.
    pub fn consume(&mut self, event: &crate::EventBody) {
        match event {
            crate::EventBody::SetProfile { profile } => self.profile = profile.clone(),
            crate::EventBody::SetWorkingDir { working_dir } => {
                self.working_dir = working_dir.clone();
            }
            crate::EventBody::CancelSchedule { .. }
            | crate::EventBody::PluginRolledBack { .. }
            | crate::EventBody::UserMessage { .. }
            | crate::EventBody::Reminder { .. }
            | crate::EventBody::Backtrack { .. }
            | crate::EventBody::SessionMessage(..)
            | crate::EventBody::ChildRunEnded { .. }
            | crate::EventBody::Compact
            | crate::EventBody::Halt(..) => {}
        }
    }

    /// Apply one fact of the Session's own Log.
    pub fn apply(&mut self, body: &crate::EntryBody) {
        if let crate::EntryBody::EventConsumed { event } = body {
            self.consume(&event.body);
        }
    }

    /// The materialized row: the creation record, the recorded archive and the Session's own Log.
    pub fn fold<'a>(
        creation: &Creation,
        archived_at: Option<DateTime<Utc>>,
        log: impl IntoIterator<Item = &'a crate::EntryBody>,
    ) -> Self {
        let mut row = Self::created(creation);
        row.archived_at = archived_at;
        for body in log {
            row.apply(body);
        }
        row
    }
}

/// Execution owner and fencing generation; a single node uses itself at epoch one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// Node currently authorized to execute this Session.
    pub node: NodeId,
    /// Ownership generation.
    pub epoch: Epoch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntryBody, Event, EventBody, EventId, EventSource, ProfileName, WorkingDir};

    fn creation() -> Creation {
        let node = NodeId::new();
        Creation {
            id: SessionId::new(),
            name: "main".parse().unwrap(),
            parent: None,
            created_at: Utc::now(),
            node,
            profile: "default".parse().unwrap(),
            working_dir: WorkingDir::new(node, "/start".into()).unwrap(),
        }
    }

    fn event(session: SessionId, body: EventBody) -> Event {
        Event {
            id: EventId::new(),
            session,
            source: EventSource::Cli,
            body,
            received_at: Utc::now(),
        }
    }

    fn profile(name: &str) -> EventBody {
        EventBody::SetProfile {
            profile: name.parse::<ProfileName>().unwrap(),
        }
    }

    fn directory(node: NodeId, path: &str) -> EventBody {
        EventBody::SetWorkingDir {
            working_dir: WorkingDir::new(node, path.into()).unwrap(),
        }
    }

    fn message() -> EventBody {
        EventBody::UserMessage {
            text: "hello".into(),
            attachments: vec![],
            unviewable: vec![],
            earlier: vec![],
            quote: None,
        }
    }

    /// The effective view is the committed fold continued over the pending Inbox, and consuming
    /// the pending Inbox makes the committed view what the effective view was, at every split.
    #[test]
    fn consuming_the_pending_inbox_commits_exactly_the_effective_view() {
        let creation = creation();
        let node = creation.node;
        let inputs: Vec<_> = [
            profile("research"),
            message(),
            directory(node, "/project"),
            profile("default"),
            message(),
            directory(node, "/other"),
        ]
        .into_iter()
        .map(|body| event(creation.id, body))
        .collect();
        for split in 0..=inputs.len() {
            let (consumed, pending) = inputs.split_at(split);
            let log: Vec<_> = consumed
                .iter()
                .map(|event| EntryBody::EventConsumed {
                    event: event.clone(),
                })
                .collect();
            let committed = SessionRecord::fold(&creation, None, &log);
            let mut effective = committed.clone();
            for event in pending {
                effective.consume(&event.body);
            }
            let all: Vec<_> = inputs
                .iter()
                .map(|event| EntryBody::EventConsumed {
                    event: event.clone(),
                })
                .collect();
            assert_eq!(SessionRecord::fold(&creation, None, &all), effective);
        }
        let last = SessionRecord::fold(
            &creation,
            None,
            &inputs
                .iter()
                .map(|event| EntryBody::EventConsumed {
                    event: event.clone(),
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(last.profile.as_str(), "default");
        assert_eq!(last.working_dir.path(), std::path::Path::new("/other"));
    }
}
