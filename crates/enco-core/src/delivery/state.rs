use crate::{EntryBody, Event, EventBody, EventSource, Halt, ReplyDestination};
use std::ops::ControlFlow;

/// What a fact, read newest first, settles about the wait for an answer.
enum Wait<'a> {
    /// An answer or a halt ends every wait before it.
    Over,
    /// This input asks for a reply, and nothing newer answered it.
    On(&'a Event),
}

fn wait_on(event: &Event) -> Option<Wait<'_>> {
    if matches!(event.body, EventBody::Halt(..)) {
        Some(Wait::Over)
    } else {
        event.is_interactive().then_some(Wait::On(event))
    }
}

fn wait(body: &EntryBody) -> Option<Wait<'_>> {
    match body {
        EntryBody::EventConsumed { event } => wait_on(event),
        body => super::content::answers(body).then_some(Wait::Over),
    }
}

/// Latest interactive input still waiting for a final reply or an unsuccessful Run result.
///
/// Mid-Run messages and skipped replies do not answer it; a consumed halt invalidates the wait.
pub fn unanswered_input(entries: &[crate::Entry]) -> Option<&Event> {
    match entries.iter().rev().find_map(|entry| wait(&entry.body))? {
        Wait::On(event) => Some(event),
        Wait::Over => None,
    }
}

/// Child facts computed from committed history followed by accepted, unconsumed inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildState {
    /// Whether the parent still waits for this child's answer.
    pub awaiting: bool,
    /// Most recent halt, provided no later input has superseded it.
    pub halted: Option<Halt>,
    /// Whether replies currently go to the owner rather than the delegating parent.
    pub replies_to_owner: bool,
}

/// Folds a child's history, newest first, into its `ChildState`.
///
/// Feed its unconsumed inputs newest first, then its Log newest first. Each fact is decided by the
/// newest fact of its kind, so the fold needs only the history since the latest input that asks
/// for a reply, and says when it has it.
#[derive(Debug, Default)]
pub struct ChildStateFold {
    awaiting: Option<bool>,
    halted: Option<Option<Halt>>,
    replies_to_owner: Option<bool>,
}

impl ChildStateFold {
    /// Fold the next older input; breaks once every fact is decided.
    pub fn input(&mut self, event: &Event) -> ControlFlow<()> {
        // A compaction request neither resumes a halted Session nor waits for a reply.
        if matches!(event.body, EventBody::Compact) {
            return ControlFlow::Continue(());
        }
        self.halted.get_or_insert_with(|| match &event.body {
            EventBody::Halt(halt) => Some(halt.clone()),
            _ => None,
        });
        if self.replies_to_owner.is_none()
            && let Some(destination) = event.reply_destination()
        {
            self.replies_to_owner = Some(matches!(
                destination,
                ReplyDestination::Cli | ReplyDestination::Channel { .. }
            ));
        }
        self.decide(wait_on(event))
    }

    /// Fold the next older Log entry; breaks once every fact is decided.
    pub fn entry(&mut self, body: &EntryBody) -> ControlFlow<()> {
        match body {
            EntryBody::EventConsumed { event } => self.input(event),
            body => self.decide(wait(body)),
        }
    }

    fn decide(&mut self, wait: Option<Wait<'_>>) -> ControlFlow<()> {
        if self.awaiting.is_none()
            && let Some(wait) = wait
        {
            self.awaiting = Some(matches!(
                wait,
                Wait::On(Event {
                    source: EventSource::Parent,
                    ..
                })
            ));
        }
        if self.awaiting.is_some() && self.halted.is_some() && self.replies_to_owner.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    /// The facts from what was folded; an undecided fact means the history never decided it.
    pub fn finish(self) -> ChildState {
        ChildState {
            awaiting: self.awaiting.unwrap_or(false),
            halted: self.halted.flatten(),
            replies_to_owner: self.replies_to_owner.unwrap_or(false),
        }
    }
}
