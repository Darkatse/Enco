use crate::*;

fn event(source: EventSource, body: EventBody) -> Event {
    Event {
        id: EventId::new(),
        session: "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap(),
        source,
        body,
        received_at: Utc::now(),
    }
}

fn message(source: EventSource) -> Event {
    event(
        source,
        EventBody::SessionMessage(Outgoing {
            text: "task".into(),
            file: None,
        }),
    )
}

fn entries(bodies: Vec<EntryBody>) -> Vec<Entry> {
    bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| Entry {
            pos: LogPos {
                epoch: Epoch(1),
                seq: Seq(index as u64 + 1),
            },
            at: Utc::now(),
            body,
        })
        .collect()
}

fn consumed(event: Event) -> EntryBody {
    EntryBody::EventConsumed { event }
}

/// Fold as the store does: unconsumed inputs newest first, then the Log newest first.
fn child_state(history: &[Entry], pending: &[Event]) -> ChildState {
    let mut fold = ChildStateFold::default();
    let inputs = pending.iter().rev().try_for_each(|event| fold.input(event));
    if inputs.is_continue() {
        let _ = history
            .iter()
            .rev()
            .try_for_each(|entry| fold.entry(&entry.body));
    }
    fold.finish()
}

fn reply_destination(history: &[Entry]) -> Option<ReplyDestination> {
    let mut projection = DeliveryProjection::default();
    for entry in history {
        projection.step(entry);
    }
    projection.destination().cloned()
}

fn halt() -> Event {
    event(
        EventSource::Parent,
        EventBody::Halt(Halt {
            root: SessionRef {
                id: SessionId::new(),
                name: "main/worker".parse().unwrap(),
            },
            by: None,
        }),
    )
}

#[test]
fn pending_and_consumed_inputs_produce_the_same_child_facts() {
    let task = message(EventSource::Parent);
    let stop = halt();
    let owner = event(
        EventSource::Cli,
        EventBody::UserMessage {
            text: "let me take over".into(),
            images: Vec::new(),
            unviewable: Vec::new(),
            earlier: Vec::new(),
            quote: None,
        },
    );
    let history = entries(vec![consumed(task.clone())]);
    assert!(child_state(&[], std::slice::from_ref(&task)).awaiting);
    for pending in [
        vec![stop.clone()],
        vec![stop, task.clone()],
        vec![owner.clone()],
        vec![owner, task],
    ] {
        let projected = child_state(&history, &pending);
        let committed = entries(
            history
                .iter()
                .map(|entry| entry.body.clone())
                .chain(pending.into_iter().map(consumed))
                .collect(),
        );
        assert_eq!(projected, child_state(&committed, &[]));
    }
}

#[test]
fn replies_halts_and_takeover_have_distinct_effects_on_the_parent_wait() {
    let task = message(EventSource::Parent);
    let mut history = entries(vec![consumed(task)]);
    let mid_run = EntryBody::ToolCallSettled {
        call: CallId::new(),
        outcome: Settlement::Ok,
        content: "sent".into(),
        full: None,
        images: Vec::new(),
        outgoing: Some(Outgoing {
            text: "progress".into(),
            file: None,
        }),
        delegated: None,
    };
    history = entries(
        history
            .into_iter()
            .map(|entry| entry.body)
            .chain([mid_run])
            .collect(),
    );
    assert!(child_state(&history, &[]).awaiting);
    history = entries(
        history
            .into_iter()
            .map(|entry| entry.body)
            .chain([EntryBody::RoundEnded {
                round: RoundId::new(),
                end: RoundEnd::Replied,
            }])
            .collect(),
    );
    assert!(!child_state(&history, &[]).awaiting);
    assert_eq!(reply_destination(&history), Some(ReplyDestination::Parent));
    let stop = halt();
    let stopped = child_state(&history, &[stop]);
    assert!(!stopped.awaiting);
    assert!(stopped.halted.is_some());
    assert!(!stopped.replies_to_owner);
    let owner = event(
        EventSource::Cli,
        EventBody::UserMessage {
            text: "continue with me".into(),
            images: Vec::new(),
            unviewable: Vec::new(),
            earlier: Vec::new(),
            quote: None,
        },
    );
    // An input that asks for a reply decides every fact, so nothing older is read.
    assert!(ChildStateFold::default().input(&owner).is_break());
    let taken_over = child_state(&history, &[owner]);
    assert!(!taken_over.awaiting);
    assert!(taken_over.replies_to_owner);
    assert!(taken_over.halted.is_none());
}

#[test]
fn noninteractive_child_reports_do_not_take_the_reply_destination() {
    let owner = event(
        EventSource::Cli,
        EventBody::UserMessage {
            text: "research this".into(),
            images: Vec::new(),
            unviewable: Vec::new(),
            earlier: Vec::new(),
            quote: None,
        },
    );
    let child = message(EventSource::Child(SessionRef {
        id: SessionId::new(),
        name: "main/worker/research".parse().unwrap(),
    }));
    assert!(!child.is_interactive());
    let history = entries(vec![consumed(owner), consumed(child), consumed(halt())]);
    assert_eq!(reply_destination(&history), Some(ReplyDestination::Cli));
    assert!(unanswered_input(&history).is_none());
}

#[test]
fn every_run_that_did_not_complete_answers_with_how_it_ended() {
    for end in [
        RunEnd::Completed,
        RunEnd::Halted,
        RunEnd::Backtracked,
        RunEnd::Cancelled,
        RunEnd::Interrupted,
        RunEnd::BudgetExhausted,
    ] {
        let history = entries(vec![
            consumed(message(EventSource::Parent)),
            EntryBody::RunEnded {
                run: RunId::new(),
                end: end.clone(),
            },
        ]);
        let boundary = project(&history).pop().unwrap();
        let handed = matches!(boundary.deliverable, Some(Deliverable::RunEnded(..)));
        assert_eq!(unanswered_input(&history).is_none(), handed);
        assert_eq!(handed, end != RunEnd::Completed);
    }
}

#[test]
fn final_reply_projection_preserves_content_and_its_parent_destination() {
    let attempt = AttemptId::new();
    let round = RoundId::new();
    let history = entries(vec![
        consumed(message(EventSource::Parent)),
        EntryBody::AttemptStarted {
            round,
            attempt,
            purpose: AttemptPurpose::Reply,
            plan: ContentHash::of(b"plan"),
            composer: GenerationId(1),
            provider: GenerationId(2),
            settings: ProviderSettings {
                base_url: "https://example.test".into(),
                model: "model".into(),
                api_key_env: None,
                options: serde_json::json!({}),
            },
        },
        EntryBody::AttemptSettled {
            attempt,
            result: AttemptResult::Completed {
                message: Message::text(Role::Assistant, "finished"),
                stop: StopReason::EndTurn,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 4,
                    cached_input_tokens: None,
                },
            },
        },
        EntryBody::RoundEnded {
            round,
            end: RoundEnd::Replied,
        },
    ]);
    let last = project(&history).pop().unwrap();
    assert_eq!(last.destination, Some(ReplyDestination::Parent));
    assert_eq!(
        last.deliverable,
        Some(Deliverable::Reply("finished".into()))
    );
    // Resuming with only the destination carried over reaches the same reply.
    let mut resumed = DeliveryProjection::resume(Some(ReplyDestination::Parent));
    let after = history[1..]
        .iter()
        .filter_map(|entry| resumed.step(entry))
        .last();
    assert_eq!(after, Some(last));
}

fn project(history: &[Entry]) -> Vec<Boundary> {
    let mut projection = DeliveryProjection::default();
    history
        .iter()
        .filter_map(|entry| projection.step(entry))
        .collect()
}
