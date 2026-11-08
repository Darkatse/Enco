use super::*;
use crate::{reach::earliest, test_support::*};

/// What every read the actor makes over `entries` gives.
fn reads(entries: &[Entry]) -> String {
    let conversation = transcript::conversation(entries);
    let last_reply = transcript::last_reply(&conversation).map(|last| {
        let (plan, usage, settings) = (last.plan, last.usage, last.settings);
        (
            last.started,
            last.settled,
            plan,
            usage,
            settings,
            last.provider,
        )
    });
    format!(
        "{:?}",
        (
            transcript::project(&conversation),
            last_reply,
            transcript::compaction_requested(&conversation, &[]),
            transcript::latest_input(&conversation),
            transcript::new_input(&conversation),
            round::run_entries(entries),
            round::this_round(entries),
            unanswered_input(entries),
            transcript::backtracked_to_input(entries),
            crate::recovery::recover(entries),
        )
    )
}

fn delivery(entries: &[Entry]) -> String {
    let mut projection = DeliveryProjection::default();
    for entry in entries {
        projection.step(entry);
    }
    format!("{projection:?}")
}

#[test]
fn reads_over_the_end_they_reach_see_what_the_whole_log_shows() {
    let history = history();
    for end in 1..=history.len() {
        let log = &history[..end];
        let (whole, folded) = (reads(log), delivery(log));
        // Wherever reading back from the end stops, by any page size.
        for read in (0..end).map(|from| &log[from..]) {
            if let Some(from) = start(read, reach(read)) {
                assert_eq!(reads(read), whole, "reading {} of {end}", read.len());
                assert_eq!(reads(&read[from..]), whole, "keeping {from}.. of {end}");
            }
            let startup = earliest([reach(read), DeliveryProjection::reach(read)]);
            if let Some(from) = start(read, startup) {
                assert_eq!(delivery(&read[from..]), folded, "folding {from}.. of {end}");
            }
        }
    }
}

#[test]
fn a_tail_kept_across_commits_sees_what_the_whole_log_shows_until_a_backtrack() {
    let history = history();
    for end in 1..=history.len() {
        let mut tail = LogTail::new(history[..end].to_vec());
        for (next, entry) in history.iter().enumerate().skip(end) {
            if tail.append(vec![entry.clone()]) {
                break;
            }
            assert_eq!(
                reads(tail.entries()),
                reads(&history[..=next]),
                "kept from {end} through {next}"
            );
        }
    }
}
