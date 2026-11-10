use super::*;
use crate::test_support::*;

fn round_started() -> EntryBody {
    EntryBody::RoundStarted {
        run: RunId::new(),
        round: RoundId::new(),
        safe_mode: false,
        working_dir: WorkingDir::new(NodeId::new(), "/tmp".into()).unwrap(),
    }
}

/// An Attempt for `purpose` on the model `model` of registration 1, with the plan named `plan`.
fn start(attempt: AttemptId, purpose: AttemptPurpose, model: &str, plan: &str) -> EntryBody {
    EntryBody::AttemptStarted {
        round: RoundId::new(),
        attempt,
        purpose,
        plan: ContentHash::of(plan.as_bytes()),
        composer: code(1),
        provider: code(1),
        settings: settings(model),
    }
}

fn refused(attempt: AttemptId) -> EntryBody {
    EntryBody::AttemptSettled {
        origin: None,
        attempt,
        result: AttemptResult::Overflow {
            overflow: Overflow {
                counted: None,
                accepted: None,
                message: "too long".into(),
            },
        },
    }
}

fn combination(model: &str, provider: u64) -> Combination {
    Combination {
        settings: settings(model),
        provider: code(provider),
    }
}

/// The plans the combination turned away in the current Round, latest first.
fn refused_plans(entries: &[Entry], model: &str, provider: u64) -> Vec<ContentHash> {
    combination(model, provider)
        .overflowed(this_round(entries))
        .into_iter()
        .map(|(plan, _)| *plan)
        .collect()
}

#[test]
fn a_combinations_refusals_in_this_round_come_latest_first() {
    let [earlier, reply, summary, other] = std::array::from_fn(|_| AttemptId::new());
    let mut bodies = vec![
        round_started(),
        start(earlier, AttemptPurpose::Reply, "a", "earlier"),
        refused(earlier),
        round_started(),
    ];
    // A new Round does not inherit the previous Round's refusals.
    assert!(refused_plans(&entries(bodies.clone()), "a", 1).is_empty());

    bodies.extend([
        start(reply, AttemptPurpose::Reply, "a", "reply"),
        refused(reply),
        start(summary, AttemptPurpose::Compaction, "a", "summary"),
        refused(summary),
        start(other, AttemptPurpose::Reply, "b", "other"),
        refused(other),
    ]);
    let of = |name: &str| ContentHash::of(name.as_bytes());
    let round = entries(bodies.clone());
    // Either purpose's refusal counts.
    assert_eq!(refused_plans(&round, "a", 1), [of("summary"), of("reply")]);
    assert_eq!(refused_plans(&round, "b", 1), [of("other")]);
    // Another generation behind the same settings is another combination.
    assert!(refused_plans(&round, "a", 2).is_empty());
    // A compaction does not end them; whether the transcript still holds a plan is checked apart.
    bodies.push(compacted(round[4].pos));
    assert_eq!(
        refused_plans(&entries(bodies), "a", 1),
        [of("summary"), of("reply")]
    );
}
