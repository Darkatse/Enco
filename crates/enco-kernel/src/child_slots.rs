use crate::session::lock;
use enco_core::SessionId;
use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

#[derive(Default)]
struct State {
    running: HashSet<SessionId>,
    waiting: VecDeque<SessionId>,
}

#[derive(Default)]
pub(crate) struct ChildSlots {
    state: Mutex<State>,
    pub changed: Notify,
}

impl ChildSlots {
    pub fn queue(self: &Arc<Self>, session: SessionId) -> Waiting {
        lock(&self.state).waiting.push_back(session);
        Waiting {
            slots: self.clone(),
            session,
        }
    }

    pub fn position(&self, session: SessionId) -> Option<usize> {
        lock(&self.state)
            .waiting
            .iter()
            .position(|id| *id == session)
            .map(|at| at + 1)
    }
}

pub(crate) struct Waiting {
    slots: Arc<ChildSlots>,
    session: SessionId,
}

impl Waiting {
    pub fn admit(&self, limit: usize) -> Option<ChildPermit> {
        let mut state = lock(&self.slots.state);
        if state.running.len() >= limit || state.waiting.front() != Some(&self.session) {
            return None;
        }
        state.waiting.pop_front();
        state.running.insert(self.session);
        self.slots.changed.notify_waiters();
        Some(ChildPermit {
            slots: self.slots.clone(),
            session: self.session,
        })
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        lock(&self.slots.state)
            .waiting
            .retain(|id| *id != self.session);
        self.slots.changed.notify_waiters();
    }
}

pub(crate) struct ChildPermit {
    slots: Arc<ChildSlots>,
    session: SessionId,
}

impl Drop for ChildPermit {
    fn drop(&mut self) {
        lock(&self.slots.state).running.remove(&self.session);
        self.slots.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_is_fifo_and_uses_the_current_limit_without_revoking_runs() {
        let slots = Arc::new(ChildSlots::default());
        let first = slots.queue(SessionId::new());
        let second = slots.queue(SessionId::new());
        let third = slots.queue(SessionId::new());
        assert!(second.admit(4).is_none());
        let running_first = first.admit(1).unwrap();
        assert!(second.admit(1).is_none());
        let running_second = second.admit(2).unwrap();
        assert!(third.admit(1).is_none());
        drop(running_first);
        assert!(third.admit(1).is_none());
        drop(running_second);
        assert!(third.admit(1).is_some());
    }

    #[test]
    fn abandoning_a_waiter_advances_its_successors() {
        let slots = Arc::new(ChildSlots::default());
        let first_id = SessionId::new();
        let next_id = SessionId::new();
        let first = slots.queue(first_id);
        let next = slots.queue(next_id);
        assert_eq!(slots.position(next_id), Some(2));
        drop(first);
        assert_eq!(slots.position(first_id), None);
        assert_eq!(slots.position(next_id), Some(1));
        let permit = next.admit(1).unwrap();
        assert_eq!(slots.position(next_id), None);
        drop(next);
        drop(permit);
    }
}
