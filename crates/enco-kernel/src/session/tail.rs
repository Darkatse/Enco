//! The end of its Log a Session actor keeps in memory, as a cache of the store's whole Log.
use crate::{
    Store, StoreError,
    reach::{earliest, read_back, start},
    round, transcript,
};
use enco_core::*;

/// The end of a Session's Log, from as far back as the actor's reads reach to the latest entry.
///
/// Every read the actor makes over it is listed in `reach`, so each gives what it would over the
/// whole Log.
#[derive(Default)]
pub(crate) struct LogTail(Vec<Entry>);

impl LogTail {
    /// Keep the end of `entries`, which reach back at least as far as the actor's reads.
    pub fn new(entries: Vec<Entry>) -> Self {
        let mut tail = Self(entries);
        tail.trim();
        tail
    }

    pub fn entries(&self) -> &[Entry] {
        &self.0
    }

    /// Add entries `session` just committed, reading its end again when they need it.
    pub async fn extend(
        &mut self,
        store: &dyn Store,
        session: SessionId,
        entries: Vec<Entry>,
    ) -> Result<(), StoreError> {
        if self.append(entries) {
            *self = Self::new(read_back(store, session, reach).await?);
        }
        Ok(())
    }

    /// Add committed entries; returns whether the end must be read again from the store.
    ///
    /// Only a consumed backtrack moves a read's reach back. Otherwise the reaches only move
    /// forward, and a Round's end drops what is behind them.
    fn append(&mut self, entries: Vec<Entry>) -> bool {
        let backtracked = entries
            .iter()
            .any(|entry| transcript::backtrack(entry).is_some());
        let round_ended = entries
            .iter()
            .any(|entry| matches!(entry.body, EntryBody::RoundEnded { .. }));
        self.0.extend(entries);
        if round_ended && !backtracked {
            self.trim();
        }
        backtracked
    }

    fn trim(&mut self) {
        if let Some(from @ 1..) = start(&self.0, reach(&self.0)) {
            self.0.drain(..from);
            self.0.shrink_to_fit();
        }
    }
}

/// How far back the actor's reads reach: those of a Round, and those between Runs.
pub(crate) fn reach(entries: &[Entry]) -> Option<LogPos> {
    let conversation = transcript::conversation(entries);
    earliest([
        transcript::conversation_reach(
            entries,
            &conversation,
            earliest([
                transcript::project_reach(&conversation),
                transcript::last_reply_reach(&conversation),
                transcript::compaction_requested_reach(&conversation),
                transcript::latest_input_reach(&conversation),
                transcript::new_input_reach(&conversation),
            ]),
        ),
        round::run_entries_reach(entries),
        round::this_round_reach(entries),
        unanswered_input_reach(entries),
        transcript::backtracked_to_input_reach(entries),
    ])
}

#[cfg(test)]
mod tests;
