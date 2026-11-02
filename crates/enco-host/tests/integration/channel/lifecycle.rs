use super::*;

#[tokio::test]
async fn archive_listing_and_invalid_child_selection_are_receipts_not_connection_failures() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("before archive"), reply("after archive")]);
    let Harness {
        kernel,
        store,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("hello".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    let original = kernel.find_session("main").await.unwrap();
    updates
        .send(vec![update(2, Action::Archive(None))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let listing = receive(&mut sending).await;
    assert!(listing.text.contains("main"));
    listing.result.send(Ok(())).unwrap();
    assert!(
        store
            .session(original.id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none()
    );
    for (offset, name) in [(3, "main/missing"), (4, "invalid name")] {
        updates
            .send(vec![update(offset, Action::SelectSession(name.into()))])
            .await
            .unwrap();
        receive(&mut polling).await;
        receive(&mut sending).await.result.send(Ok(())).unwrap();
        assert!(matches!(
            channel.status().await.unwrap().state,
            ChannelState::Running
        ));
    }
    assert!(matches!(
        kernel.find_session("main/missing").await,
        Err(enco_kernel::KernelError::Lookup(
            enco_kernel::SessionLookupError::Unknown(_)
        ))
    ));
    kernel.archive(original.id, EventSource::Cli).await.unwrap();
    updates
        .send(vec![update(
            5,
            Action::Message("route after archive".into()),
        )])
        .await
        .unwrap();
    receive(&mut polling).await;
    let mut responses = Vec::new();
    for _ in 0..2 {
        let sent = receive(&mut sending).await;
        responses.push(sent.text);
        sent.result.send(Ok(())).unwrap();
    }
    // The owner is told where the conversation goes now, besides the reply itself.
    assert_eq!(
        responses
            .iter()
            .filter(|text| *text != "after archive")
            .count(),
        1
    );
    assert_ne!(kernel.find_session("main").await.unwrap().id, original.id);
    assert!(matches!(
        channel.status().await.unwrap().state,
        ChannelState::Running
    ));
    channel.shutdown().await.unwrap();
    // Restoring a connection that followed archived identities must also remain healthy.
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(6, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(matches!(
        channel.status().await.unwrap().state,
        ChannelState::Running
    ));
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn named_archive_stops_following_and_halt_without_a_route_only_reports_absence() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("first")]);
    let Harness {
        kernel,
        store,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Halt(None))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(kernel.sessions().await.unwrap().is_empty());
    updates
        .send(vec![update(2, Action::Message("hello".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    let main = kernel.find_session("main").await.unwrap();
    updates
        .send(vec![update(3, Action::Halt(None))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(store.pending(main.id).await.unwrap().is_empty());
    assert!(
        store
            .session(main.id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none()
    );
    updates
        .send(vec![update(4, Action::Archive(Some("main".into())))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(
        store
            .session(main.id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_some()
    );
    let other = kernel.open_session("other").await.unwrap();
    updates
        .send(vec![update(5, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    let listing = receive(&mut sending).await;
    assert!(listing.text.contains(other.name.as_str()));
    assert!(!listing.text.contains(main.name.as_str()));
    listing.result.send(Ok(())).unwrap();
    assert!(matches!(
        channel.status().await.unwrap().state,
        ChannelState::Running
    ));
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

async fn prompt(queue: &mut mpsc::UnboundedReceiver<Prompt>) -> Prompt {
    tokio::time::timeout(Duration::from_secs(15), queue.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn owner_takes_over_a_child_in_a_conversation_without_waking_its_parent() {
    let dir = tempfile::tempdir().unwrap();
    let mut document = configuration();
    document["endpoint"]["worker"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["worker"]["model"] = "worker".into();
    document["profile"]["worker"] = json!({"reply": ["worker"], "compaction": ["compaction"]});
    let (to_fixture, mut fixture) = mpsc::unbounded_channel();
    let (to_worker, mut worker) = mpsc::unbounded_channel();
    let queues = ProviderQueues(std::collections::HashMap::from([
        ("fixture".to_owned(), to_fixture),
        ("worker".to_owned(), to_worker),
    ]));
    let (kernel, store) =
        kernel_configured(dir.path(), Arc::new(queues), document, vec![], |_| {}).await;
    let Harness {
        kernel,
        store,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = controlled(kernel, store);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("delegate".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let task = json!({"to": "research", "text": "task", "profile": "worker"});
    prompt(&mut fixture)
        .await
        .respond(calls(vec![call("session_send", &task.to_string())]));
    let child_first = prompt(&mut worker).await;
    prompt(&mut fixture).await.respond(reply("delegated"));
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    child_first.respond(reply("for main"));
    prompt(&mut fixture)
        .await
        .respond(crate::silence::skip("noted"));
    updates
        .send(vec![update(
            2,
            Action::SelectSession("main/research".into()),
        )])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    updates
        .send(vec![update(3, Action::Message("hi child".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    prompt(&mut worker).await.respond(reply("for owner"));
    // The child's reply comes back to this conversation, as from the Session it routes to.
    let sent = receive(&mut sending).await;
    assert_eq!(sent.text, "for owner");
    sent.result.send(Ok(())).unwrap();
    // The reply went to the owner in the same commit that ended the Round, so the parent holds
    // only the report from before the takeover.
    let main = kernel.find_session("main").await.unwrap();
    let mut reports = store.pending(main.id).await.unwrap();
    reports.extend(
        kernel
            .log(main.id, None)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|entry| match entry.body {
                EntryBody::EventConsumed { event } => Some(event),
                _ => None,
            }),
    );
    assert_eq!(
        reports
            .iter()
            .filter(|event| matches!(event.source, EventSource::Child(_)))
            .count(),
        1
    );
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn halting_a_stopped_session_answers_the_owner_and_keeps_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = factory_runtime(fixture(
        ScriptedProvider::new(vec![]),
        ScriptedProvider::new(vec![]),
    ));
    let (development, registry, store) =
        scripted_home(dir.path(), runtime, configuration(), vec![]).await;
    let broken = store
        .ensure_session(&"broken".parse().unwrap(), Utc::now())
        .await
        .unwrap();
    // A future Log body is valid JSON but unreadable, so this Session's actor stops on start.
    rusqlite::Connection::open(dir.path().join("enco.db"))
        .unwrap()
        .execute(
            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,1,1,?,?)",
            rusqlite::params![
                broken.id.to_string(),
                broken.created_at.to_rfc3339(),
                r#"{"kind":"future_record"}"#
            ],
        )
        .unwrap();
    let deps = kernel_deps(dir.path(), store.clone(), registry, None, Some(development)).await;
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = controlled(Kernel::start(deps).await.unwrap(), store.clone());
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Halt(Some("broken".into())))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(matches!(
        channel.status().await.unwrap().state,
        ChannelState::Running
    ));
    // The halt stays recorded for when the Session runs again.
    assert!(
        store
            .pending(broken.id)
            .await
            .unwrap()
            .iter()
            .any(|event| matches!(event.body, EventBody::Halt(_)))
    );
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
