//! What the owner reads about one Session: where it stands, its children, and how much context
//! its conversation uses. Node-wide facts stay in `Kernel::status`.
use super::*;

/// One Session as its owner reads it, from its actor, its Log and the running configuration.
#[derive(Debug, Clone)]
pub struct SessionOverview {
    /// Durable Session metadata.
    pub session: SessionRecord,
    /// The first reply model its profile configures; absent when the profile is not in the
    /// running configuration.
    pub reply: Option<ReplyModel>,
    /// Number of its unarchived direct children.
    pub children: usize,
    /// Number of those children running a Run now.
    pub running_children: usize,
    /// Where it stands between and within Runs.
    pub run: RunState,
    /// How much context its current conversation uses.
    pub context: ContextUsage,
}

/// A profile's first reply model, where its next request goes unless that model fails.
#[derive(Debug, Clone)]
pub struct ReplyModel {
    /// The model the endpoint runs.
    pub model: String,
    /// The endpoint's context window in tokens.
    pub window: std::num::NonZeroU32,
}

/// Where a Session stands between and within Runs.
#[derive(Debug, Clone)]
pub enum RunState {
    /// Its actor stopped after a storage failure.
    Stopped {
        /// Recorded failure message.
        reason: String,
    },
    /// A Run is executing.
    Running {
        /// When the Run started.
        started_at: DateTime<Utc>,
        /// The Round it is in, starting at one.
        round: usize,
    },
    /// Waiting for a child Run slot.
    Waiting {
        /// FIFO position, starting at one.
        position: usize,
    },
    /// Halted, by the same rule a parent uses for its children.
    Halted(Halt),
    /// Idle between Runs.
    Idle {
        /// When the latest Run ended, and how; absent before any Run has ended.
        last_run: Option<(DateTime<Utc>, RunEnd)>,
    },
}

/// How much context a Session's current conversation uses, as recorded.
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// The latest completed reply request in the conversation, if any.
    pub last_request: Option<LastRequest>,
    /// How many compactions the conversation records.
    pub compactions: usize,
    /// Whether a compaction request waits for the next Run.
    pub compaction_requested: bool,
}

/// The latest completed reply request in a conversation, as the service reported it.
#[derive(Debug, Clone)]
pub struct LastRequest {
    /// The model that served it.
    pub model: String,
    /// The token usage the service reported.
    pub usage: Usage,
    /// Whether a compaction was recorded after it, so its size no longer describes the
    /// conversation.
    pub compacted_since: bool,
}

impl Kernel {
    /// The owner's view of one Session.
    pub async fn session_overview(&self, id: SessionId) -> Result<SessionOverview, KernelError> {
        let session = self.session(id).await?;
        let reply = self
            .deps
            .registry
            .view()
            .config
            .kernel
            .profiles
            .get(&session.profile)
            .map(|profile| {
                let first = profile.reply.first();
                ReplyModel {
                    model: first.endpoint.settings.model.clone(),
                    window: first.budget.context_tokens,
                }
            });
        let children = self.deps.store.children(id).await?;
        let entries = self.deps.store.log(id, None).await?;
        let pending = self.deps.store.pending(id).await?;
        let (status, running_children) = {
            let handles = lock(&self.sessions);
            let running_children = children
                .iter()
                .filter(|child| super::running(&handles, child.id))
                .count();
            (self.session_status(&handles, session), running_children)
        };
        let conversation = crate::transcript::conversation(&entries);
        Ok(SessionOverview {
            run: run_state(&entries, &pending, &status),
            context: context_usage(&conversation, &pending),
            session: status.session,
            reply,
            children: children.len(),
            running_children,
        })
    }
}

fn run_state(entries: &[Entry], pending: &[Event], status: &SessionStatus) -> RunState {
    if let Some(reason) = &status.stopped {
        return RunState::Stopped {
            reason: reason.clone(),
        };
    }
    if status.running {
        let mut round = 0;
        for entry in entries.iter().rev() {
            match entry.body {
                EntryBody::RoundStarted { .. } => round += 1,
                EntryBody::RunStarted { .. } => {
                    return RunState::Running {
                        started_at: entry.at,
                        round: round.max(1),
                    };
                }
                EntryBody::RunEnded { .. } => break,
                _ => {}
            }
        }
        // Admission precedes RunStarted's commit; until then only earlier facts are reportable.
    }
    if let Some(position) = status.waiting {
        return RunState::Waiting { position };
    }
    let mut fold = ChildStateFold::default();
    if pending
        .iter()
        .rev()
        .try_for_each(|event| fold.input(event))
        .is_continue()
    {
        let _ = entries
            .iter()
            .rev()
            .try_for_each(|entry| fold.entry(&entry.body));
    }
    if let Some(halt) = fold.finish().halted {
        return RunState::Halted(halt);
    }
    RunState::Idle {
        last_run: entries.iter().rev().find_map(|entry| match &entry.body {
            EntryBody::RunEnded { end, .. } => Some((entry.at, end.clone())),
            _ => None,
        }),
    }
}

fn context_usage(conversation: &[&Entry], pending: &[Event]) -> ContextUsage {
    ContextUsage {
        last_request: last_request(conversation),
        compactions: conversation
            .iter()
            .filter(|entry| matches!(entry.body, EntryBody::Compacted { .. }))
            .count(),
        compaction_requested: crate::transcript::compaction_requested(conversation, pending),
    }
}

fn last_request(conversation: &[&Entry]) -> Option<LastRequest> {
    let mut completed = HashMap::new();
    let mut compacted_since = false;
    for entry in conversation.iter().rev() {
        match &entry.body {
            EntryBody::Compacted { .. } => compacted_since = true,
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { usage, .. },
            } => {
                completed.insert(attempt, (usage, compacted_since));
            }
            EntryBody::AttemptStarted {
                attempt,
                purpose: AttemptPurpose::Reply,
                settings,
                ..
            } => {
                if let Some((usage, compacted_since)) = completed.remove(attempt) {
                    return Some(LastRequest {
                        model: settings.model.clone(),
                        usage: usage.clone(),
                        compacted_since,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
