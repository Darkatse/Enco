//! Following Sessions: their Logs projected for views and epilogues, the child Sessions found in
//! them, and letting go of archived ones.
use super::*;
use enco_kernel::{
    ChannelActivity, ChannelActivityEpilogue, ChannelToolActivity, ChannelToolState,
};
use serde_json::Map;
use std::collections::{HashMap, hash_map::Entry as PlanEntry};

/// What the connection knows of one followed Session from its Log.
#[derive(Default)]
pub(super) struct Observation {
    pub cursor: Option<LogPos>,
    pub destination: Option<ReplyDestination>,
    /// Set for a Session found as a child in its parent's Log.
    pub child_of: Option<ChildOf>,
    pub archived: bool,
    pub runs: BTreeMap<RunId, ObservedRun>,
    current: Option<RunId>,
}
/// The parent a child Session works for, and the child's full name.
pub(super) struct ChildOf {
    pub parent: SessionId,
    pub name: SessionName,
}

pub(super) struct ObservedRun {
    pub id: RunId,
    pub started: DateTime<Utc>,
    pub ended: Option<(LogPos, ChannelActivityEpilogue)>,
    reply_attempt: Option<AttemptId>,
    pending_excerpt: String,
    rounds: u32,
    unsent: bool,
    excerpt: String,
    plan: Option<ContentHash>,
    proposed: BTreeMap<CallId, Map<String, Value>>,
    tools: Vec<ObservedTool>,
}
struct ObservedTool {
    call: CallId,
    activity: ChannelToolActivity,
    arguments: Option<(ContentHash, Map<String, Value>)>,
}

impl Connection {
    pub(super) async fn observe(&mut self, session: SessionId) -> Result<(), ChannelError> {
        let mut pending = std::collections::VecDeque::from([session]);
        while let Some(session) = pending.pop_front() {
            let observation = self.observations.entry(session).or_default();
            let restoring = observation.cursor.is_none();
            let mut children = Vec::new();
            for entry in self.kernel.log(session, observation.cursor).await? {
                observation.cursor = Some(entry.pos);
                if let EntryBody::ToolCallSettled {
                    outcome: Settlement::Ok,
                    delegated: Some(delegated),
                    ..
                } = &entry.body
                {
                    children.push(delegated.session.clone());
                }
                observation.apply(entry);
            }
            // Runs already projected before a restart need no new view without a saved card.
            let processed = self
                .state
                .outbound
                .get(&session)
                .and_then(|cursor| cursor.processed);
            if restoring {
                observation.runs.retain(|id, run| {
                    run.ended
                        .as_ref()
                        .is_none_or(|(pos, _)| Some(*pos) > processed)
                        || self.state.activity.contains_key(id)
                });
            }
            for child in children {
                if !self.state.outbound.contains_key(&child.id) {
                    // An archived child without a cursor has nothing left here: none of its replies
                    // came to this connection, and no conversation of its own is open.
                    if self.kernel.session(child.id).await?.archived_at.is_some() {
                        continue;
                    }
                    let mut state = self.state.clone();
                    state.outbound.insert(child.id, Cursor::default());
                    self.commit(state, None).await?;
                }
                self.observations.entry(child.id).or_default().child_of = Some(ChildOf {
                    parent: session,
                    name: child.name,
                });
                self.follow(child.id).await?;
                pending.push_back(child.id);
            }
        }
        Ok(())
    }

    /// Whether `session` has no Run left to show and no reply left to deliver here.
    pub(super) fn finished(&self, session: SessionId) -> bool {
        self.observations
            .get(&session)
            .is_some_and(|observation| observation.runs.is_empty())
            && !self.dirty.contains(&session)
            && self
                .in_flight
                .as_ref()
                .is_none_or(|flight| flight.delivery.session != session)
    }

    /// Stop following the archived Sessions finished here, once any conversation opened for them
    /// is put away.
    ///
    /// One still bound to them is closed first (`conversation_requests`); one the owner bound
    /// elsewhere stays theirs.
    pub(super) async fn retire(&mut self) -> Result<(), ChannelError> {
        let retiring: Vec<_> = self
            .observations
            .iter()
            .filter(|(session, observation)| {
                observation.archived
                    && !self.views.conversation_busy.contains(*session)
                    && self.finished(**session)
                    && !matches!(
                        self.state.children.get(*session),
                        Some(Creation::Shown(target)) if self.state.chats.get(target) == Some(*session)
                    )
            })
            .map(|(session, _)| *session)
            .collect();
        if retiring.is_empty() {
            return Ok(());
        }
        let mut state = self.state.clone();
        for session in &retiring {
            state.children.remove(session);
            state.outbound.remove(session);
        }
        self.commit(state, None).await?;
        for session in retiring {
            self.observations.remove(&session);
        }
        Ok(())
    }
}

impl Observation {
    fn apply(&mut self, entry: enco_core::Entry) {
        match entry.body {
            EntryBody::EventConsumed { event } => {
                if let Some(destination) = event.reply_destination() {
                    self.destination = Some(destination);
                }
            }
            EntryBody::RunStarted { run } => {
                self.current = Some(run);
                self.runs.insert(
                    run,
                    ObservedRun {
                        id: run,
                        started: entry.at,
                        ended: None,
                        reply_attempt: None,
                        pending_excerpt: String::new(),
                        rounds: 0,
                        unsent: false,
                        excerpt: String::new(),
                        plan: None,
                        proposed: BTreeMap::new(),
                        tools: Vec::new(),
                    },
                );
            }
            _ => {
                if let Some(run) = self.current.and_then(|id| self.runs.get_mut(&id)) {
                    run.apply(entry);
                }
            }
        }
    }
}

impl ObservedRun {
    fn apply(&mut self, entry: enco_core::Entry) {
        match entry.body {
            EntryBody::AttemptStarted {
                purpose,
                attempt,
                plan,
                ..
            } => {
                self.reply_attempt = (purpose == AttemptPurpose::Reply).then_some(attempt);
                if purpose == AttemptPurpose::Reply {
                    self.plan = Some(plan);
                    self.pending_excerpt.clear();
                }
            }
            EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed { message, .. },
                ..
            } if self.reply_attempt == Some(attempt) => {
                self.pending_excerpt = shorten(
                    message
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            Part::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .flat_map(str::lines)
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .unwrap_or("")
                        .chars(),
                );
                self.proposed = message
                    .tool_calls()
                    .filter_map(|call| match Arguments::parse(&call.arguments) {
                        Arguments::Object(arguments) => Some((call.id, arguments)),
                        Arguments::Invalid { .. } => None,
                    })
                    .collect();
            }
            EntryBody::RoundStarted { .. } => {
                self.rounds += 1;
                self.reply_attempt = None;
                self.pending_excerpt.clear();
            }
            EntryBody::RoundEnded { end, .. } => {
                if matches!(end, RoundEnd::Replied | RoundEnd::ReplySkipped) {
                    self.unsent = end == RoundEnd::ReplySkipped;
                    self.excerpt = std::mem::take(&mut self.pending_excerpt);
                }
            }
            EntryBody::RunEnded { end, .. } => {
                self.ended = Some((
                    entry.pos,
                    ChannelActivityEpilogue {
                        elapsed_seconds: elapsed(self.started, entry.at),
                        rounds: self.rounds,
                        end,
                        unsent: self.unsent,
                        excerpt: self.excerpt.clone(),
                    },
                ));
            }
            EntryBody::ToolCallStarted {
                call, capability, ..
            } => {
                let arguments = self.plan.zip(self.proposed.remove(&call));
                self.tools.push(ObservedTool {
                    call,
                    arguments,
                    activity: ChannelToolActivity {
                        name: capability.name,
                        state: ChannelToolState::Running,
                        arguments: String::new(),
                    },
                });
                let excess = self
                    .tools
                    .len()
                    .saturating_sub(crate::limits::CHANNEL_ACTIVITY_TOOLS);
                self.tools.drain(..excess);
            }
            EntryBody::ToolCallSettled { call, outcome, .. } => {
                if let Some(tool) = self.tools.iter_mut().find(|tool| tool.call == call) {
                    tool.activity.state = match outcome {
                        Settlement::Ok => ChannelToolState::Ok,
                        Settlement::Failed { .. } => ChannelToolState::Failed,
                        Settlement::Unknown { .. } => ChannelToolState::Unknown,
                    };
                }
            }
            _ => {}
        }
    }

    pub(super) async fn view(
        &mut self,
        kernel: &Kernel,
        plans: &mut HashMap<ContentHash, ContextPlan>,
        now: DateTime<Utc>,
    ) -> Result<ChannelActivity, ChannelError> {
        fill_arguments(kernel, plans, self).await?;
        Ok(ChannelActivity {
            elapsed_seconds: elapsed(self.started, now),
            tools: self
                .tools
                .iter()
                .map(|tool| tool.activity.clone())
                .collect(),
        })
    }
}

fn elapsed(start: DateTime<Utc>, end: DateTime<Utc>) -> u32 {
    u32::try_from((end - start).num_seconds().max(0)).unwrap_or(u32::MAX)
}

/// `chars` on one line of at most `CHANNEL_ACTIVITY_LINE_CHARS` characters, ending in `…` when
/// it is cut; it stops reading once the line is full.
fn shorten(chars: impl Iterator<Item = char>) -> String {
    let mut chars = chars.peekable();
    let mut line: String = chars
        .by_ref()
        .take(crate::limits::CHANNEL_ACTIVITY_LINE_CHARS)
        .collect();
    if chars.peek().is_some() {
        line.pop();
        line.push('…');
    }
    line
}

/// Fill in the arguments of the Run's tools that still hold them, just before a view shows them;
/// `plans` keeps each plan read once per refresh.
async fn fill_arguments(
    kernel: &Kernel,
    plans: &mut HashMap<ContentHash, ContextPlan>,
    run: &mut ObservedRun,
) -> Result<(), ChannelError> {
    for tool in &mut run.tools {
        let Some((hash, arguments)) = tool.arguments.take() else {
            continue;
        };
        let plan = match plans.entry(hash) {
            PlanEntry::Occupied(entry) => entry.into_mut(),
            PlanEntry::Vacant(entry) => entry.insert(kernel.plan(&hash).await?),
        };
        tool.activity.arguments = argument_line(&arguments, &required(plan, &tool.activity.name));
    }
    Ok(())
}

/// The arguments `tool` must be given, in the order its disclosed schema lists them.
fn required<'a>(plan: &'a ContextPlan, tool: &str) -> Vec<&'a str> {
    plan.tools
        .iter()
        .find(|(_, spec)| spec.name == tool)
        .and_then(|(_, spec)| spec.input_schema.get("required")?.as_array())
        .map_or_else(Vec::new, |names| {
            names.iter().filter_map(Value::as_str).collect()
        })
}

/// The values of a call's `required` arguments, their words joined by single spaces on one
/// shortened line; optional arguments are left out.
fn argument_line(arguments: &Map<String, Value>, required: &[&str]) -> String {
    let values: Vec<_> = required
        .iter()
        .filter_map(|name| arguments.get(*name))
        .map(|value| match value {
            Value::String(text) => std::borrow::Cow::Borrowed(text.as_str()),
            other => std::borrow::Cow::Owned(other.to_string()),
        })
        .collect();
    let words = values.iter().flat_map(|value| value.split_whitespace());
    shorten(
        words
            .enumerate()
            .flat_map(|(index, word)| (index > 0).then_some(' ').into_iter().chain(word.chars())),
    )
}

#[cfg(test)]
mod tests {
    use super::argument_line;
    use serde_json::json;

    #[test]
    fn argument_lines_follow_the_required_order_and_leave_optional_arguments_out() {
        let arguments = json!({
            "content": "fn main() {}\n".repeat(10),
            "path": "src/main.rs",
            "timeout_ms": 5,
        });
        let arguments = arguments.as_object().unwrap();
        let written = argument_line(arguments, &["path", "content"]);
        assert!(written.starts_with("src/main.rs fn main() {} fn main"));
        assert_eq!(written.chars().count(), 60);
        assert!(written.ends_with('…'));
        assert_eq!(argument_line(arguments, &[]), "");
    }
}
