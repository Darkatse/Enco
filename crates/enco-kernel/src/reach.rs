//! Reading the end of a Session's Log only as far back as the reads over it reach.
//!
//! Every read over a Log looks back from its end to an entry the kernel recorded, such as the
//! current Run's start or the latest compaction, and needs nothing before it. Each read says how
//! far with its `_reach` function: the entry it stops at, or `None` when it needs all it was given.
use crate::{Store, StoreError, limits::LOG_PAGE};
use enco_core::*;

/// How far back several reads reach together; `None` when one of them needs all it was given.
pub(crate) fn earliest(reaches: impl IntoIterator<Item = Option<LogPos>>) -> Option<LogPos> {
    // `None` orders first, so one read that needs everything makes them all need it.
    reaches.into_iter().min().flatten()
}

/// Where reads that reach back to `reach` start in `entries`; `None` when they may start before
/// them.
pub(crate) fn start(entries: &[Entry], reach: Option<LogPos>) -> Option<usize> {
    let reach = reach?;
    (entries.first()?.pos <= reach).then(|| entries.partition_point(|entry| entry.pos < reach))
}

/// Read the end of `session`'s Log back as far as `reach` says the reads over it reach, or all of
/// it when they may reach before its start; returns everything read.
///
/// Each further request asks for as many entries as were read, so the store is asked about
/// log(n) times for n entries. The Log only grows at its end, so the entries before a position
/// never change between requests.
pub(crate) async fn read_back(
    store: &dyn Store,
    session: SessionId,
    reach: impl Fn(&[Entry]) -> Option<LogPos>,
) -> Result<Vec<Entry>, StoreError> {
    let mut entries: Vec<Entry> = Vec::new();
    loop {
        let limit = entries.len().max(LOG_PAGE);
        let before = entries.first().map(|entry| entry.pos);
        let mut earlier = store.log_before(session, before, limit).await?;
        let whole = earlier.len() < limit;
        earlier.append(&mut entries);
        entries = earlier;
        if whole || start(&entries, reach(&entries)).is_some() {
            return Ok(entries);
        }
    }
}
