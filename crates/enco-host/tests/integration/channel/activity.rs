use super::*;

#[tokio::test]
async fn progress_tracks_tools_and_shows_presence_until_the_reply_deletes_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("message_send", r#"{"text":"working"}"#),
            call("shell_exec", r#"{"command":"read line < gate"}"#),
        ]),
        reply("finished"),
    ]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut activity,
        mut presence,
        ..
    } = harness(dir.path(), provider).await;
    adapter.activity_enabled.store(true, Ordering::SeqCst);
    adapter.menu_failure.store(true, Ordering::SeqCst);
    let gate = dir.path().join("workspace/gate");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&gate)
            .status()
            .unwrap()
            .success()
    );
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    assert!(channel.status().await.unwrap().menu_failure.is_some());
    updates
        .send(vec![update(1, Action::Message("work".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let created = receive(&mut activity).await;
    assert_eq!(created.target, "chat");
    assert!(created.shown.is_none());
    // Presence starts with the Run, beside the card's creation.
    assert_eq!(receive(&mut presence).await, "chat");
    created.result.send(Ok(Some("card".into()))).unwrap();
    let progress = receive(&mut sending).await;
    assert_eq!(progress.text, "working");
    tokio::time::pause();
    // The connection observes the shell call on its own cursor; refresh until the card shows it.
    let refreshed = loop {
        tokio::time::advance(Duration::from_secs(4)).await;
        let refreshed = receive(&mut activity).await;
        assert_eq!(refreshed.shown.as_deref(), Some("card"));
        assert_eq!(receive(&mut presence).await, "chat");
        let tools = &refreshed.view().unwrap().tools;
        if tools.iter().any(|tool| tool.name == "shell_exec") {
            break refreshed;
        }
        refreshed.result.send(Ok(None)).unwrap();
    };
    assert!(
        refreshed
            .view()
            .unwrap()
            .tools
            .iter()
            .any(|tool| tool.name == "shell_exec"
                && tool.arguments == "read line < gate"
                && matches!(tool.state, enco_kernel::ChannelToolState::Running))
    );
    tokio::time::resume();
    refreshed.result.send(Ok(None)).unwrap();
    progress.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut entries = kernel.subscribe(session.id).await.unwrap();
    tokio::fs::write(&gate, b"done\n").await.unwrap();
    finish(&mut entries).await;
    let replacement = start(&kernel, &adapter);
    receive(&mut polling).await;
    let reply = receive(&mut sending).await;
    assert_eq!(reply.text, "finished");
    assert!(activity.try_recv().is_err());
    reply.result.send(Ok(())).unwrap();
    let deleted = receive(&mut activity).await;
    assert_eq!(deleted.shown.as_deref(), Some("card"));
    assert!(deleted.view().is_none());
    deleted.result.send(Ok(None)).unwrap();
    replacement.shutdown().await.unwrap();
    // The Run had ended before the replacement started, so it shows no presence.
    assert!(presence.try_recv().is_err());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn answers_wait_for_acknowledgment_without_blocking_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    let (acknowledged, gate) = oneshot::channel();
    *adapter.acknowledgment.lock().await = Some(gate);
    let mut command = update(1, Action::Sessions);
    command.input.respond = Some("interaction".into());
    let key = command.key.clone();
    updates.send(vec![command]).await.unwrap();
    receive(&mut polling).await;
    assert!(adapter.answers.lock().await.is_empty());
    updates
        .send(vec![update(2, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    let ordinary = receive(&mut sending).await;
    ordinary.result.send(Ok(())).unwrap();
    assert!(adapter.answers.lock().await.is_empty());
    acknowledged.send(()).unwrap();
    adapter.answer_notify.notified().await;
    assert_eq!(
        adapter.answers.lock().await[0],
        (
            "interaction".into(),
            Some("No Sessions yet. Send a message to open main.".into())
        )
    );
    let mut duplicate = update(3, Action::Sessions);
    duplicate.key = key;
    duplicate.input.respond = Some("interaction".into());
    let mut forbidden = update(4, Action::Cancel);
    forbidden.input.respond = Some("forbidden".into());
    forbidden.input.sender = Some("stranger".into());
    updates.send(vec![duplicate, forbidden]).await.unwrap();
    receive(&mut polling).await;
    adapter.answer_notify.notified().await;
    let answers = adapter.answers.lock().await;
    assert_eq!(answers.len(), 2);
    assert_eq!(
        answers[1],
        (
            "forbidden".into(),
            Some("Only the owner can use this.".into())
        )
    );
    drop(answers);
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_drains_creation_and_replacement_does_not_repeat_failed_or_unknown_creation() {
    for unknown in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let provider = ScriptedProvider::new(vec![
            calls(vec![call(
                "shell_exec",
                r#"{"command":"read line < gate"}"#,
            )]),
            reply("finished"),
        ]);
        let Harness {
            kernel,
            adapter,
            updates,
            mut polling,
            mut sending,
            mut activity,
            ..
        } = harness(dir.path(), provider).await;
        adapter.activity_enabled.store(true, Ordering::SeqCst);
        let gate = dir.path().join("workspace/gate");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&gate)
                .status()
                .unwrap()
                .success()
        );
        let channel = Arc::new(start(&kernel, &adapter));
        receive(&mut polling).await;
        updates
            .send(vec![update(1, Action::Message("work".into()))])
            .await
            .unwrap();
        receive(&mut polling).await;
        let created = receive(&mut activity).await;
        let shutdown = tokio::spawn({
            let channel = channel.clone();
            async move { channel.shutdown().await }
        });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        let mut fault = lost_response();
        fault.unknown = unknown;
        fault.failure.retryable = false;
        created.result.send(Err(fault)).unwrap();
        shutdown.await.unwrap().unwrap();
        let replacement = start(&kernel, &adapter);
        receive(&mut polling).await;
        updates
            .send(vec![update(2, Action::Sessions)])
            .await
            .unwrap();
        receive(&mut polling).await;
        receive(&mut sending).await.result.send(Ok(())).unwrap();
        assert!(activity.try_recv().is_err());
        tokio::fs::write(&gate, b"done\n").await.unwrap();
        let reply = receive(&mut sending).await;
        assert_eq!(reply.text, "finished");
        reply.result.send(Ok(())).unwrap();
        replacement.shutdown().await.unwrap();
        assert!(activity.try_recv().is_err());
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_presence_only_view_cancels_without_creating_or_removing_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let (send, mut prompts) = mpsc::unbounded_channel();
    let provider = Arc::new(ProviderQueues(std::collections::HashMap::from([(
        "fixture".into(),
        send,
    )])));
    let (kernel, store) = kernel(dir.path(), provider).await;
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut activity,
        mut presence,
        ..
    } = controlled(kernel, store);
    adapter.activity_enabled.store(true, Ordering::SeqCst);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("work".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let _prompt = receive(&mut prompts).await;
    let draft = receive(&mut activity).await;
    assert!(draft.replies_here);
    draft.result.send(Ok(None)).unwrap();
    assert_eq!(receive(&mut presence).await, "chat");
    let session = kernel.find_session("main").await.unwrap().id;
    let mut entries = kernel.subscribe(session).await.unwrap();
    updates.send(vec![update(2, Action::Cancel)]).await.unwrap();
    receive(&mut polling).await;
    let log = finish(&mut entries).await;
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    for _ in 0..2 {
        receive(&mut sending).await.result.send(Ok(())).unwrap();
    }
    channel.shutdown().await.unwrap();
    assert!(activity.try_recv().is_err());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_group_card_retries_creation_and_removal_after_the_protocols_retry_after() {
    let dir = tempfile::tempdir().unwrap();
    let (send, mut prompts) = mpsc::unbounded_channel();
    let provider = Arc::new(ProviderQueues(std::collections::HashMap::from([(
        "fixture".into(),
        send,
    )])));
    let mut document = configuration();
    document["profile"]["group"] = json!({"reply":["reply"],"compaction":["compaction"],"tools":[],"owner_context":false,"requires_lifeline":false});
    let (kernel, store) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut activity,
        ..
    } = controlled(kernel, store);
    adapter.activity_enabled.store(true, Ordering::SeqCst);
    let mut policy = settings();
    policy.group_profile = Some("group".parse().unwrap());
    let channel = start_with(&kernel, &adapter, policy);
    receive(&mut polling).await;
    let mut input = update(1, Action::Message("work".into()));
    input.input.direct = false;
    updates.send(vec![input]).await.unwrap();
    receive(&mut polling).await;
    let prompt = receive(&mut prompts).await;
    let create = receive(&mut activity).await;
    assert!(create.replies_here);
    tokio::time::pause();
    create.result.send(Err(limited())).unwrap();
    tokio::time::advance(Duration::from_secs(9)).await;
    let create = receive(&mut activity).await;
    assert!(create.shown.is_none());
    create.result.send(Ok(Some("card".into()))).unwrap();
    prompt.respond(reply("answer"));
    let answer = receive(&mut sending).await;
    answer.result.send(Ok(())).unwrap();
    let remove = loop {
        let view = receive(&mut activity).await;
        if matches!(view.update, ChannelActivityUpdate::Remove) {
            break view;
        }
        view.result.send(Ok(None)).unwrap();
    };
    remove.result.send(Err(limited())).unwrap();
    tokio::time::advance(Duration::from_secs(9)).await;
    let remove = receive(&mut activity).await;
    assert!(matches!(remove.update, ChannelActivityUpdate::Remove));
    assert_eq!(remove.shown.as_deref(), Some("card"));
    remove.result.send(Ok(None)).unwrap();
    tokio::time::resume();
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
