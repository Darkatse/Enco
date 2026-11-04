//! Conversation commands exercise the real acceptance boundary and the next Run's composition.
use super::*;
use enco_kernel::{ComposeInput, Composer, ComposerOptions, Composition};

pub(super) async fn command(h: &mut Harness, input: Update) -> (String, String) {
    h.updates.send(vec![input]).await.unwrap();
    receive(&mut h.polling).await;
    let sent = receive(&mut h.sending).await;
    sent.result.send(Ok(())).unwrap();
    (sent.text, sent.id)
}

/// Waits until the Session consumes a backtrack; its receipt only says it was accepted.
async fn backtracked(entries: &mut tokio::sync::broadcast::Receiver<Entry>) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !matches!(
            entries.recv().await.unwrap().body,
            EntryBody::EventConsumed { event } if matches!(event.body, EventBody::Backtrack { .. })
        ) {}
    })
    .await
    .unwrap();
}

#[derive(Default)]
struct RequestedComposer(std::sync::Mutex<Vec<ComposeInput>>);

#[async_trait]
impl Composer for RequestedComposer {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        self.0.lock().unwrap().push(input.clone());
        if input.compaction_requested
            && let (Some(compaction), Some(upto)) =
                (&input.compaction, input.transcript.round_ends.last())
        {
            return Ok(Composition::Compact {
                upto: *upto,
                plan: enco_kernel::CompositionPlan {
                    items: vec![enco_kernel::CompositionItem::Message {
                        role: Role::User,
                        text: "summarize".into(),
                        sources: vec![],
                    }],
                    tools: vec![],
                    max_output_tokens: Some(compaction.budget.max_output_tokens.get()),
                    omitted: vec![],
                },
            });
        }
        Ok(Composition::Plan(reply_plan(input)))
    }
}

#[tokio::test]
async fn main_commands_work_before_routing_and_empty_conversations_do_not_request_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    // A command never creates a Session, `main` included.
    let (status, _) = command(&mut h, update(0, Action::Status)).await;
    assert_eq!(
        status,
        "This conversation has no Session yet; its first message starts one."
    );
    assert!(h.kernel.sessions().await.unwrap().is_empty());
    h.kernel.open_session("main").await.unwrap();
    let (status, _) = command(&mut h, update(1, Action::Status)).await;
    assert!(status.starts_with("main · profile default · fixture\nIdle; no Run has ended yet"));
    assert!(status.contains("Context: no requests in this conversation yet"));
    assert!(status.contains("Enco test-version (test-revision)"));
    let (compact, _) = command(&mut h, update(2, Action::Compact)).await;
    assert_eq!(compact, "Nothing to compact yet.");
    let (clear, _) = command(&mut h, update(3, Action::Clear)).await;
    assert!(clear.contains("/backtrack on the last reply"));
    let (compact, _) = command(&mut h, update(4, Action::Compact)).await;
    assert_eq!(compact, "Nothing to compact yet.");
    channel.shutdown().await.unwrap();
    let session = h.kernel.open_session("main").await.unwrap();
    let log = h.kernel.log(session.id, None).await.unwrap();
    assert!(!log.iter().any(|entry| matches!(&entry.body, EntryBody::EventConsumed { event } if matches!(event.body, EventBody::Compact))));
    assert!(
        !h.kernel
            .session_overview(session.id)
            .await
            .unwrap()
            .context
            .compaction_requested
    );
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn clear_preserves_identity_and_backtrack_to_the_previous_reply_does_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("first answer"), reply("fresh answer")]);
    let mut h = harness(dir.path(), provider.clone()).await;
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    let (_, reply_id) = command(&mut h, update(1, Action::Message("before clear".into()))).await;
    let before = h.kernel.open_session("main").await.unwrap();
    let mut entries = h.kernel.subscribe(before.id).await.unwrap();
    command(&mut h, update(2, Action::Clear)).await;
    backtracked(&mut entries).await;
    let after = h.kernel.session(before.id).await.unwrap();
    assert_eq!(after, before);
    let mut back = update(3, Action::Backtrack);
    back.input.reply_to = Some(reply_id);
    command(&mut h, back).await;
    backtracked(&mut entries).await;
    let (status, _) = command(&mut h, update(4, Action::Status)).await;
    assert!(status.contains("tokens in the last request"));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    command(&mut h, update(5, Action::Clear)).await;
    backtracked(&mut entries).await;
    let (status, _) = command(&mut h, update(6, Action::Status)).await;
    assert!(status.contains("Context: no requests in this conversation yet"));
    command(&mut h, update(7, Action::Message("after clear".into()))).await;
    channel.shutdown().await.unwrap();
    {
        let requests = provider.requests.lock().unwrap();
        let text = requests[1]
            .messages
            .iter()
            .map(Message::joined_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("before clear") && !text.contains("first answer"));
        assert!(text.contains("after clear"));
    }
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_command_after_its_session_is_archived_says_where_the_conversation_goes_now() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("answer")]);
    let mut h = harness(dir.path(), provider).await;
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    h.kernel.open_session("main").await.unwrap();
    command(&mut h, update(1, Action::SelectSession("work".into()))).await;
    let work = h.kernel.find_session("work").await.unwrap();
    h.kernel.archive(work.id, EventSource::Cli).await.unwrap();
    let (cleared, _) = command(&mut h, update(2, Action::Clear)).await;
    assert!(cleared.starts_with(
        "Session work is archived; this conversation now goes to Session main.\nCleared"
    ));
    // The clear went to `main`, whether or not its actor has consumed it yet.
    let main = h.kernel.open_session("main").await.unwrap();
    let pending = h.store.pending(main.id).await.unwrap();
    let log = h.kernel.log(main.id, None).await.unwrap();
    let consumed = log.iter().filter_map(|entry| match &entry.body {
        EntryBody::EventConsumed { event } => Some(event),
        _ => None,
    });
    assert!(
        pending
            .iter()
            .chain(consumed)
            .any(|event| event.body == EventBody::Backtrack { to: None })
    );
    let (answer, _) = command(&mut h, update(3, Action::Message("hello".into()))).await;
    assert_eq!(answer, "answer");
    channel.shutdown().await.unwrap();
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn compaction_is_pending_until_the_first_round_of_the_next_run_and_clear_discards_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut measured = reply("first answer").unwrap();
    measured.usage = Usage {
        input_tokens: 61_200,
        output_tokens: 20,
        cached_input_tokens: Some(56_304),
    };
    let provider = ScriptedProvider::new(vec![
        Ok(measured),
        reply("summary"),
        calls(vec![call("fs_read", r#"{"path":"note.txt"}"#)]),
        reply("after tool"),
        reply("fresh"),
    ]);
    let composer = Arc::new(RequestedComposer::default());
    let (kernel, store) = kernel_configured_composer(
        dir.path(),
        provider.clone(),
        configuration(),
        vec![],
        composer.clone(),
        |_| {},
    )
    .await;
    std::fs::write(dir.path().join("workspace/note.txt"), "note").unwrap();
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    command(&mut h, update(1, Action::Message("first input".into()))).await;
    let (receipt, _) = command(&mut h, update(2, Action::Compact)).await;
    assert_eq!(
        receipt,
        "The conversation will be compacted when the next Run starts."
    );
    let (status, _) = command(&mut h, update(3, Action::Status)).await;
    assert!(status.contains("Context: 61.2k of 128k tokens in the last request (47%), 92% cached"));
    assert!(status.contains("compaction requested for the next Run"));
    command(&mut h, update(4, Action::Message("continue".into()))).await;
    let (status, _) = command(&mut h, update(5, Action::Status)).await;
    assert!(status.contains("compacted once"));
    assert!(!status.contains("compaction requested"));
    {
        let inputs = composer.0.lock().unwrap();
        let requested: Vec<_> = inputs
            .iter()
            .filter(|input| input.compaction_requested)
            .collect();
        assert_eq!(requested.len(), 1);
        assert_eq!(requested[0].transcript.round_ends.len(), 1);
    }
    command(&mut h, update(6, Action::Compact)).await;
    command(&mut h, update(7, Action::Clear)).await;
    command(
        &mut h,
        update(8, Action::Message("new conversation".into())),
    )
    .await;
    channel.shutdown().await.unwrap();
    assert_eq!(
        composer
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|input| input.compaction_requested)
            .count(),
        1
    );
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn commands_in_unrouted_groups_or_topics_never_create_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    for direct in [true, false] {
        for (offset, action) in [
            (1, Action::Status),
            (2, Action::Compact),
            (3, Action::Clear),
        ] {
            let mut input = update(offset, action);
            input.input.direct = direct;
            input.input.realm = Some("realm".into());
            let (text, _) = command(&mut h, input).await;
            assert_eq!(
                text,
                if direct {
                    "This conversation has no Session yet; its first message starts one."
                } else {
                    "Set group_profile for this connection to use the Agent in group chats."
                }
            );
        }
    }
    channel.shutdown().await.unwrap();
    assert!(h.kernel.sessions().await.unwrap().is_empty());
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn compact_during_a_run_waits_for_the_next_run_without_interrupting_work() {
    let dir = tempfile::tempdir().unwrap();
    let gate = super::Gate::new("main");
    let provider = ScriptedProvider::new(vec![
        reply("current answer"),
        reply("summary"),
        reply("next answer"),
    ]);
    let composer = Arc::new(RequestedComposer::default());
    let (kernel, store) = kernel_configured_composer(
        dir.path(),
        provider.clone(),
        configuration(),
        vec![],
        composer.clone(),
        |deps| deps.memories = gate.clone(),
    )
    .await;
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    h.updates
        .send(vec![update(1, Action::Message("current work".into()))])
        .await
        .unwrap();
    receive(&mut h.polling).await;
    gate.entered.notified().await;
    command(&mut h, update(2, Action::Compact)).await;
    let (status, _) = command(&mut h, update(3, Action::Status)).await;
    assert!(
        status.contains("Running since ")
            && status.contains("compaction requested for the next Run")
    );
    assert!(provider.requests.lock().unwrap().is_empty());
    gate.release.notify_one();
    let answer = receive(&mut h.sending).await;
    assert_eq!(answer.text, "current answer");
    answer.result.send(Ok(())).unwrap();
    command(&mut h, update(4, Action::Message("next work".into()))).await;
    channel.shutdown().await.unwrap();
    {
        let inputs = composer.0.lock().unwrap();
        assert!(!inputs[0].compaction_requested);
        assert_eq!(
            inputs
                .iter()
                .filter(|input| input.compaction_requested)
                .count(),
            1
        );
    }
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn clear_stops_the_run_at_a_boundary_and_supersedes_unanswered_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let gate = super::Gate::new("main");
    let provider = ScriptedProvider::new(vec![reply("fresh answer")]);
    let (kernel, store) = kernel_with(dir.path(), provider.clone(), Tz::UTC, |deps| {
        deps.memories = gate.clone()
    })
    .await;
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    h.updates
        .send(vec![update(1, Action::Message("old work".into()))])
        .await
        .unwrap();
    receive(&mut h.polling).await;
    gate.entered.notified().await;
    h.updates
        .send(vec![
            update(2, Action::Message("pending input".into())),
            update(3, Action::Clear),
        ])
        .await
        .unwrap();
    receive(&mut h.polling).await;
    let mut texts = Vec::new();
    for _ in 0..2 {
        let sent = receive(&mut h.sending).await;
        texts.push(sent.text);
        sent.result.send(Ok(())).unwrap();
    }
    assert!(
        texts
            .iter()
            .any(|text| text == "Run stopped to go back in the conversation.")
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("/backtrack on the last reply"))
    );
    let (status, _) = command(&mut h, update(4, Action::Status)).await;
    assert!(
        status.contains("Idle since ") && status.contains("no requests in this conversation yet")
    );
    command(&mut h, update(5, Action::Message("fresh input".into()))).await;
    channel.shutdown().await.unwrap();
    {
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let text = requests[0]
            .messages
            .iter()
            .map(Message::joined_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("old work") && !text.contains("pending input"));
        assert!(text.contains("fresh input"));
    }
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn status_keeps_the_measured_reply_after_compaction_and_names_its_original_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut measured = reply("measured reply").unwrap();
    measured.usage = Usage {
        input_tokens: 450,
        output_tokens: 20,
        cached_input_tokens: Some(0),
    };
    let provider = ScriptedProvider::new(vec![
        Ok(measured),
        reply("summary"),
        Err(Failure {
            code: code::PROVIDER_SERVER.into(),
            message: "service unavailable".into(),
            retryable: false,
        }),
    ]);
    let mut document = configuration();
    document["run"]["max_rounds"] = 1.into();
    document["endpoint"]["alternate"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["alternate"]["model"] = "alternate".into();
    document["endpoint"]["alternate"]["window_tokens"] = 64_000.into();
    document["profile"]["other"] = json!({"reply": ["alternate"], "compaction": ["compaction"]});
    let (kernel, store) = kernel_configured_composer(
        dir.path(),
        provider,
        document,
        vec![],
        Arc::new(RequestedComposer::default()),
        |_| {},
    )
    .await;
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    command(&mut h, update(1, Action::Message("measure this".into()))).await;
    let session = h.kernel.open_session("main").await.unwrap();
    h.kernel
        .set_profile(session.id, &"other".parse().unwrap())
        .await
        .unwrap();
    command(&mut h, update(2, Action::Compact)).await;
    command(&mut h, update(3, Action::Message("continue".into()))).await;
    let (text, _) = command(&mut h, update(4, Action::Status)).await;
    assert!(text.starts_with("main · profile other · alternate"));
    assert!(text.contains("last Run failed: service unavailable"));
    assert!(text.contains("Context: 450 of 64k tokens in the last request (0%), 0% cached (on fixture); compacted since that request; compacted once"));
    channel.shutdown().await.unwrap();
    h.kernel.shutdown().await.unwrap();
}
