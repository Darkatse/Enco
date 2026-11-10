//! Status receipts expose execution and node facts without creating model requests.
use super::conversation::command;
use super::*;
use enco_kernel::RunState;

#[tokio::test]
async fn status_shows_running_halted_and_safe_mode_without_requesting_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let gate = super::Gate::new("main");
    let provider = ScriptedProvider::new(vec![reply("answer")]);
    let (kernel, store) = kernel_with(dir.path(), provider.clone(), Tz::Asia__Shanghai, |deps| {
        deps.memories = gate.clone()
    })
    .await;
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    h.updates
        .send(vec![update(1, Action::Message("work".into()))])
        .await
        .unwrap();
    receive(&mut h.polling).await;
    gate.entered.notified().await;
    let (status, _) = command(&mut h, update(2, Action::Status)).await;
    assert!(status.contains("Running since ") && status.contains(", Round 1"));
    assert!(provider.requests.lock().unwrap().is_empty());
    let session = h.kernel.open_session("main").await.unwrap();
    h.kernel.halt(session.id, EventSource::Cli).await.unwrap();
    let ended = receive(&mut h.sending).await;
    assert_eq!(ended.text, "Run halted.");
    ended.result.send(Ok(())).unwrap();
    let (status, _) = command(&mut h, update(3, Action::Status)).await;
    assert!(status.contains("Halted by the owner"));
    h.store.set_safe_mode(true).await.unwrap();
    let (status, _) = command(&mut h, update(4, Action::Status)).await;
    assert!(status.contains("Safe mode is on; run enco status for details."));
    channel.shutdown().await.unwrap();
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_group_status_and_clear_are_limited_to_its_own_session() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("private answer"), reply("group answer")]);
    let mut document = configuration();
    document["profile"]["group"] = json!({"reply": ["reply"], "compaction": ["compaction"], "tools": [], "owner_context": false, "requires_lifeline": false});
    let (kernel, store) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    let mut h = controlled(kernel, store);
    let mut policy = settings();
    policy.group_profile = Some("group".parse().unwrap());
    let channel = start_with(&h.kernel, &h.adapter, policy);
    receive(&mut h.polling).await;
    let main = h.kernel.open_session("main").await.unwrap();
    let mut entries = h.kernel.subscribe(main.id).await.unwrap();
    command(&mut h, update(1, Action::Message("private input".into()))).await;
    finish(&mut entries).await;
    let before = h.kernel.log(main.id, None).await.unwrap();
    for (offset, action) in [
        (2, Action::Message("group input".into())),
        (3, Action::Status),
        (4, Action::Clear),
    ] {
        let status = matches!(action, Action::Status);
        let mut input = update(offset, action);
        input.input.direct = false;
        input.input.conversation = "group".into();
        let (text, _) = command(&mut h, input).await;
        if status {
            assert!(text.contains("profile group") && text.contains("Working directory:"));
            assert!(!text.contains("Enco ") && !text.contains("main ·"));
        }
    }
    channel.shutdown().await.unwrap();
    assert_eq!(h.kernel.log(main.id, None).await.unwrap(), before);
    let group = h
        .kernel
        .sessions()
        .await
        .unwrap()
        .into_iter()
        .find(|session| session.id != main.id)
        .unwrap();
    // The receipt answers once the clear is accepted; the group consumes it when its Run stops.
    let mut group_entries = h.kernel.subscribe(group.id).await.unwrap();
    while !h.store.pending(group.id).await.unwrap().is_empty() {
        tokio::time::timeout(Duration::from_secs(15), group_entries.recv())
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        h.kernel
            .session_overview(group.id)
            .await
            .unwrap()
            .context
            .last_request
            .is_none()
    );
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_storage_failure_stops_only_the_session_and_is_visible_in_status() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let session = h.kernel.open_session("main").await.unwrap();
    let db = rusqlite::Connection::open(store_paths(dir.path()).db).unwrap();
    db.execute_batch("CREATE TRIGGER fail_log BEFORE INSERT ON log BEGIN SELECT RAISE(FAIL, 'storage unavailable'); END;").unwrap();
    h.kernel
        .submit(session.id, EventId::new(), "work".into(), vec![])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(
                h.kernel.session_overview(session.id).await.unwrap().run,
                RunState::Stopped { .. }
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    let (text, _) = command(&mut h, update(1, Action::Status)).await;
    assert!(text.contains("Stopped after a failure:") && text.contains("storage unavailable"));
    assert!(text.contains(
        "Session main stopped: storage: storage unavailable; run enco status for details."
    ));
    channel.shutdown().await.unwrap();
    db.execute_batch("DROP TRIGGER fail_log;").unwrap();
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn status_reports_a_child_waiting_for_its_execution_slot() {
    let dir = tempfile::tempdir().unwrap();
    let (parents, mut parent) = mpsc::unbounded_channel();
    let (children, mut child_prompts) = mpsc::unbounded_channel();
    let provider = Arc::new(ProviderQueues(std::collections::HashMap::from([
        ("fixture".into(), parents),
        ("worker".into(), children),
    ])));
    let mut document = configuration();
    document["run"]["max_concurrent_children"] = 1.into();
    document["endpoint"]["worker"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["worker"]["model"] = "worker".into();
    document["profile"]["worker"] = json!({"reply": ["worker"], "compaction": ["compaction"]});
    let (kernel, store) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    let mut h = controlled(kernel, store);
    let channel = start(&h.kernel, &h.adapter);
    receive(&mut h.polling).await;
    h.updates
        .send(vec![update(1, Action::Message("delegate".into()))])
        .await
        .unwrap();
    receive(&mut h.polling).await;
    receive(&mut parent).await.respond(calls(vec![call(
        "session_send",
        r#"{"to":"first","text":"task","profile":"worker"}"#,
    )]));
    let first = receive(&mut child_prompts).await;
    receive(&mut parent).await.respond(calls(vec![call(
        "session_send",
        r#"{"to":"second","text":"task","profile":"worker"}"#,
    )]));
    receive(&mut parent).await.respond(reply("delegated"));
    receive(&mut h.sending).await.result.send(Ok(())).unwrap();
    let child = h.kernel.find_session("main/second").await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(
                h.kernel.session_overview(child.id).await.unwrap().run,
                RunState::Waiting { position: 1 }
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    command(&mut h, update(2, Action::Clear)).await;
    let (text, _) = command(&mut h, update(3, Action::Status)).await;
    assert!(text.contains("2 children, 1 running"));
    let main = h.kernel.open_session("main").await.unwrap();
    assert_eq!(h.store.children(main.id).await.unwrap().len(), 2);
    command(
        &mut h,
        update(4, Action::SelectSession("main/second".into())),
    )
    .await;
    let (text, _) = command(&mut h, update(5, Action::Status)).await;
    assert!(text.starts_with("main/second ·"));
    assert!(text.contains("Waiting its turn to run (position 1)"));
    h.kernel.halt(main.id, EventSource::Cli).await.unwrap();
    drop(first);
    channel.shutdown().await.unwrap();
    h.kernel.shutdown().await.unwrap();
}
