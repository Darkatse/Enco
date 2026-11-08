//! What the owner reads about one Session: where it stands, its children, and how much context
//! its conversation uses. Node-wide facts stay in `Kernel::status`.
use super::*;
use crate::reach::earliest;

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
    /// The endpoint's configured name.
    pub endpoint: String,
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
    /// Whether a compaction request waits for the next Run.
    pub compaction_requested: bool,
    /// The input limit the first reply model's service gave when it last turned a request away, a
    /// reply or a summary.
    ///
    /// Present only when that limit and the output budget fall short of the configured window, so
    /// the conversation is turned away each time it grows past the limit, and no later answered
    /// request carried more input than the limit allows.
    pub service_limit: Option<u64>,
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
            .map(|profile| profile.reply.first().clone());
        let children = self.deps.store.children(id).await?;
        let entries = crate::reach::read_back(self.deps.store.as_ref(), id, |end| {
            reach(end, reply.as_ref())
        })
        .await?;
        let pending = self.deps.store.pending(id).await?;
        let halted = self.deps.store.child_state(id).await?.halted;
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
            run: run_state(&entries, halted, &status),
            context: context_usage(&conversation, &pending, reply.as_ref()),
            session: status.session,
            reply: reply.map(|first| ReplyModel {
                endpoint: first.endpoint.name.clone(),
                model: first.endpoint.settings.model.clone(),
                window: first.budget.context_tokens,
            }),
            children: children.len(),
            running_children,
        })
    }
}

/// How far back the overview reads: what `run_state` and `context_usage` read.
fn reach(entries: &[Entry], reply: Option<&CompletionEndpoint>) -> Option<LogPos> {
    let conversation = crate::transcript::conversation(entries);
    let reads = [
        crate::transcript::last_reply_reach(&conversation),
        crate::transcript::compaction_requested_reach(&conversation),
    ];
    let limit = reply.map(|reply| service_limit_reach(&conversation, reply));
    earliest([
        run_state_reach(entries),
        crate::transcript::conversation_reach(
            entries,
            &conversation,
            earliest(reads.into_iter().chain(limit)),
        ),
    ])
}

/// `halted` is the Session's halt as its parent would read it.
fn run_state(entries: &[Entry], halted: Option<Halt>, status: &SessionStatus) -> RunState {
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
    if let Some(halt) = halted {
        return RunState::Halted(halt);
    }
    RunState::Idle {
        last_run: last_run(entries).map(|(entry, end)| (entry.at, end.clone())),
    }
}

/// How far back `run_state` reads: to the latest Run end, where both counting the running Run's
/// Rounds and finding the last Run stop; `None` when no Run has ended.
fn run_state_reach(entries: &[Entry]) -> Option<LogPos> {
    last_run(entries).map(|(entry, _)| entry.pos)
}

fn last_run(entries: &[Entry]) -> Option<(&Entry, &RunEnd)> {
    entries.iter().rev().find_map(|entry| match &entry.body {
        EntryBody::RunEnded { end, .. } => Some((entry, end)),
        _ => None,
    })
}

fn context_usage(
    conversation: &[&Entry],
    pending: &[Event],
    reply: Option<&CompletionEndpoint>,
) -> ContextUsage {
    ContextUsage {
        last_request: crate::transcript::last_reply(conversation).map(|last| LastRequest {
            model: last.settings.model.clone(),
            usage: last.usage.clone(),
            compacted_since: conversation.iter().any(|entry| {
                entry.pos > last.settled && matches!(entry.body, EntryBody::Compacted { .. })
            }),
        }),
        compaction_requested: crate::transcript::compaction_requested(conversation, pending),
        service_limit: reply.and_then(|reply| service_limit(conversation, reply)),
    }
}

/// The input limit the reply endpoint's service gave when it last turned a request away.
///
/// A limit is the service's, so a summary refused or answered with the same call parameters
/// counts as a reply does. Only a limit that, with the output budget, falls short of the
/// configured window counts, and only until an answered request carries more input than it.
fn service_limit(conversation: &[&Entry], reply: &CompletionEndpoint) -> Option<u64> {
    let refusal = refusal(conversation, reply)?;
    let room = refusal
        .accepted
        .saturating_add(reply.budget.max_output_tokens.get().into());
    (refusal.answered <= refusal.accepted && room < reply.budget.context_tokens.get().into())
        .then_some(refusal.accepted)
}

/// How far back `service_limit` reads the conversation: to the latest refused request that gave
/// a limit; `None` when the service never gave one.
fn service_limit_reach(conversation: &[&Entry], reply: &CompletionEndpoint) -> Option<LogPos> {
    refusal(conversation, reply).map(|refusal| refusal.started)
}

/// The latest request the reply endpoint's service turned away with a limit.
struct Refusal {
    /// Where the request started.
    started: LogPos,
    /// The input limit the service gave.
    accepted: u64,
    /// The most input a later answered request carried, as the service counted it.
    answered: u64,
}

fn refusal(conversation: &[&Entry], reply: &CompletionEndpoint) -> Option<Refusal> {
    let mut settled = HashMap::new();
    let mut answered = 0;
    for entry in conversation.iter().rev() {
        match &entry.body {
            EntryBody::AttemptSettled {
                attempt, result, ..
            } => {
                settled.insert(*attempt, result);
            }
            EntryBody::AttemptStarted {
                attempt, settings, ..
            } if *settings == reply.endpoint.settings => match settled.remove(attempt) {
                Some(AttemptResult::Completed { usage, .. }) => {
                    answered = answered.max(usage.input_tokens);
                }
                Some(AttemptResult::Overflow {
                    overflow:
                        Overflow {
                            accepted: Some(accepted),
                            ..
                        },
                }) => {
                    return Some(Refusal {
                        started: entry.pos,
                        accepted: *accepted,
                        answered,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
