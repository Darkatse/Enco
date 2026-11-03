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
    let context = context_usage(&crate::transcript::conversation(&history), &[]);
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
    let context = context_usage(&crate::transcript::conversation(&history), &[]);
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
    let context = context_usage(&crate::transcript::conversation(&history), &[]);
    assert!(context.last_request.is_none());
    assert_eq!(context.compactions, 0);
}
