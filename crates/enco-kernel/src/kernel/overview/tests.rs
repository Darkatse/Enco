use super::*;
use crate::test_support::*;

/// The live execution facts `Kernel::status` reports for a Session.
fn status(running: bool, waiting: Option<usize>, stopped: Option<&str>) -> SessionStatus {
    let node = NodeId::new();
    SessionStatus {
        session: SessionRecord {
            id: session_id(),
            name: "main".parse().unwrap(),
            parent: None,
            created_at: Utc::now(),
            archived_at: None,
            binding: Binding {
                node,
                epoch: Epoch(1),
            },
            profile: ProfileName::default(),
            working_dir: WorkingDir::new(node, "/tmp".into()).unwrap(),
        },
        running,
        waiting,
        stopped: stopped.map(Into::into),
    }
}

#[test]
fn execution_state_prioritizes_failure_running_and_waiting_over_halt() {
    let run = RunId::new();
    let round = RoundId::new();
    // The halt is superseded by the input the later Run answered.
    let mut history = entries(vec![
        consumed(halt()),
        EntryBody::RunStarted { run },
        message(),
        EntryBody::RoundStarted {
            run,
            round,
            safe_mode: false,
            working_dir: WorkingDir::new(NodeId::new(), "/tmp".into()).unwrap(),
        },
        round_end(),
        EntryBody::RoundStarted {
            run,
            round: RoundId::new(),
            safe_mode: false,
            working_dir: WorkingDir::new(NodeId::new(), "/tmp".into()).unwrap(),
        },
    ]);
    assert!(matches!(
        run_state(&history, &[], &status(true, Some(1), Some("disk"))),
        RunState::Stopped { .. }
    ));
    assert!(
        matches!(run_state(&history, &[], &status(true, Some(1), None)),
        RunState::Running { started_at, round: 2 } if started_at == history[1].at)
    );
    assert!(matches!(
        run_state(&history, &[], &status(false, Some(3), None)),
        RunState::Waiting { position: 3 }
    ));
    history.push(Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(7),
        },
        at: Utc::now(),
        body: EntryBody::RunEnded {
            run,
            end: RunEnd::Completed,
        },
    });
    assert!(matches!(
        run_state(&history, &[], &status(false, None, None)),
        RunState::Idle {
            last_run: Some((_, RunEnd::Completed))
        }
    ));
    let halted = [event(halt())];
    assert!(matches!(
        run_state(&history, &halted, &status(false, None, None)),
        RunState::Halted(_)
    ));
    let compact = [event(halt()), event(EventBody::Compact)];
    assert!(matches!(
        run_state(&history, &compact, &status(false, None, None)),
        RunState::Halted(_)
    ));
    let newer = [
        event(halt()),
        event(EventBody::UserMessage {
            text: "continue".into(),
            attachments: vec![],
            unviewable: vec![],
            earlier: vec![],
            quote: None,
        }),
    ];
    assert!(matches!(
        run_state(&history, &newer, &status(false, None, None)),
        RunState::Idle { .. }
    ));
}

#[test]
fn admission_does_not_invent_a_start_or_reuse_an_ended_run() {
    assert!(matches!(
        run_state(&[], &[], &status(true, None, None)),
        RunState::Idle { last_run: None }
    ));
    let run = RunId::new();
    let history = entries(vec![
        EntryBody::RunStarted { run },
        round_end(),
        EntryBody::RunEnded {
            run,
            end: RunEnd::Completed,
        },
    ]);
    assert!(matches!(
        run_state(&history, &[], &status(true, None, None)),
        RunState::Idle {
            last_run: Some((_, RunEnd::Completed))
        }
    ));
    let pending = [event(halt())];
    assert!(matches!(
        run_state(&history, &pending, &status(true, None, None)),
        RunState::Halted(_)
    ));
}

#[test]
fn context_uses_completed_reply_measurements_and_follows_the_current_branch() {
    let reply = AttemptId::new();
    let compaction = AttemptId::new();
    let failed = AttemptId::new();
    let mut history = entries(vec![
        message(),
        started(reply, AttemptPurpose::Reply, "measured-model"),
        completed(reply, 1234),
        round_end(),
        consumed(EventBody::Compact),
        started(compaction, AttemptPurpose::Compaction, "summary-model"),
        completed(compaction, 999),
    ]);
    history.push(Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(8),
        },
        at: Utc::now(),
        body: compacted(history[3].pos),
    });
    let mut tail = entries(vec![
        started(failed, AttemptPurpose::Reply, "failed-model"),
        EntryBody::AttemptSettled {
            origin: None,
            attempt: failed,
            result: AttemptResult::Failed {
                failure: Failure {
                    code: "test".into(),
                    message: "failed".into(),
                    retryable: false,
                },
            },
        },
    ]);
    for entry in &mut tail {
        entry.pos.seq.0 += 8;
    }
    history.extend(tail);
    let context = context_usage(&crate::transcript::conversation(&history), &[], None);
    let last = context.last_request.unwrap();
    assert_eq!(last.model, "measured-model");
    assert_eq!(last.usage.input_tokens, 1234);
    assert!(last.compacted_since);
    assert_eq!(context.compactions, 1);
    assert!(!context.compaction_requested);

    history.push(Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(11),
        },
        at: Utc::now(),
        body: consumed(EventBody::Backtrack {
            to: Some(history[2].pos),
        }),
    });
    let context = context_usage(&crate::transcript::conversation(&history), &[], None);
    assert!(!context.last_request.unwrap().compacted_since);
    assert_eq!(context.compactions, 0);
    history.push(Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(12),
        },
        at: Utc::now(),
        body: consumed(EventBody::Backtrack { to: None }),
    });
    let context = context_usage(&crate::transcript::conversation(&history), &[], None);
    assert!(context.last_request.is_none());
    assert_eq!(context.compactions, 0);
}

fn endpoint(model: &str, window: u32, max_output: u32) -> CompletionEndpoint {
    CompletionEndpoint {
        endpoint: crate::settings::Endpoint {
            name: "chat".into(),
            plugin: "fixture".parse().unwrap(),
            settings: settings(model),
        },
        budget: crate::Budget {
            context_tokens: window.try_into().unwrap(),
            max_output_tokens: max_output.try_into().unwrap(),
        },
        accepts: Default::default(),
    }
}

fn overflowed(attempt: AttemptId, accepted: Option<u64>) -> EntryBody {
    EntryBody::AttemptSettled {
        origin: None,
        attempt,
        result: AttemptResult::Overflow {
            overflow: Overflow {
                counted: None,
                accepted,
                message: "too long".into(),
            },
        },
    }
}

#[test]
fn service_limit_appears_only_when_the_reply_service_accepts_less_than_its_window() {
    let older = AttemptId::new();
    let latest = AttemptId::new();
    let bodies = vec![
        message(),
        started(older, AttemptPurpose::Reply, "chat-model"),
        overflowed(older, Some(128_000)),
        started(latest, AttemptPurpose::Reply, "chat-model"),
        // A refusal without a limit says nothing about the window; the older one still does.
        overflowed(latest, None),
    ];
    let limit = |bodies: &[EntryBody], reply: &CompletionEndpoint| {
        let history = entries(bodies.to_vec());
        let conversation = crate::transcript::conversation(&history);
        context_usage(&conversation, &[], Some(reply)).service_limit
    };
    let reply = endpoint("chat-model", 1_000_000, 65_536);
    assert_eq!(limit(&bodies, &reply), Some(128_000));
    // 128,000 accepted plus 8,192 of output covers a 136,192-token window.
    assert_eq!(
        limit(&bodies, &endpoint("chat-model", 136_192, 8_192)),
        None
    );
    assert_eq!(
        limit(&bodies, &endpoint("chat-model", 136_193, 8_192)),
        Some(128_000)
    );
    // A refusal by other call parameters does not describe this endpoint.
    assert_eq!(
        limit(&bodies, &endpoint("other-model", 1_000_000, 65_536)),
        None
    );
    let history = entries(bodies.clone());
    let conversation = crate::transcript::conversation(&history);
    assert_eq!(context_usage(&conversation, &[], None).service_limit, None);

    // A later answer that carried more input than the limit allows disproves it.
    let answered = |input_tokens| {
        let attempt = AttemptId::new();
        let mut later = bodies.clone();
        later.extend([
            started(attempt, AttemptPurpose::Reply, "chat-model"),
            completed(attempt, input_tokens),
        ]);
        later
    };
    assert_eq!(limit(&answered(128_000), &reply), Some(128_000));
    assert_eq!(limit(&answered(128_001), &reply), None);

    // The limit is the service's: a summary it turned away with the same call parameters counts.
    let summary = AttemptId::new();
    let refused_summary = [
        message(),
        started(summary, AttemptPurpose::Compaction, "chat-model"),
        overflowed(summary, Some(128_000)),
    ];
    assert_eq!(limit(&refused_summary, &reply), Some(128_000));
}
