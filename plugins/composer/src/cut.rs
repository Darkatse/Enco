//! Where a request may cut its history: only at Round ends, so a call stays with its results, and
//! never into the part that must stay.
use crate::composer::*;
use crate::key;

/// The Round ends at which history may be cut, each with the estimated history after it, in Log
/// order.
///
/// `sizes` and `history_tokens` measure the history as rendered. Only ends before `keep_from`
/// qualify: the item there and everything after it stay.
pub(super) fn cuts<'a>(
    transcript: &'a Transcript,
    sizes: &'a [(LogPos, u32)],
    history_tokens: u32,
    keep_from: LogPos,
) -> impl Iterator<Item = (LogPos, u32)> + 'a {
    let mut tail = history_tokens;
    let mut sizes = sizes.iter().peekable();
    transcript
        .round_ends
        .iter()
        .take_while(move |end| key(end) < key(&keep_from))
        .map(move |end| {
            while let Some((_, size)) = sizes.next_if(|(pos, _)| key(pos) <= key(end)) {
                tail = tail.saturating_sub(*size);
            }
            (*end, tail)
        })
}

/// Where this Run's visible history begins: the first item after the latest visible Run end.
///
/// Leaving history out drops it without a trace, and this Run's calls and results record effects
/// that have already happened, so only earlier Runs may be left out. With no Run end visible,
/// every visible item belongs to this Run.
pub(super) fn run_start(transcript: &Transcript) -> Option<LogPos> {
    let ended = transcript.run_ends.last().map(|ended| key(&ended.pos));
    transcript
        .items
        .iter()
        .find(|item| ended.is_none_or(|ended| key(&item.pos) > ended))
        .map(|item| item.pos)
}
