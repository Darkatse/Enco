mod throttle;
use super::*;
use std::collections::HashMap;

struct Delegation {
    host: Harness,
    parent: mpsc::UnboundedReceiver<Prompt>,
    child: mpsc::UnboundedReceiver<Prompt>,
}
async fn delegated(root: &Path, max_children: u32) -> Delegation {
    let (parents, parent) = mpsc::unbounded_channel();
    let (children, child) = mpsc::unbounded_channel();
    let provider = Arc::new(ProviderQueues(HashMap::from([
        ("fixture".into(), parents),
        ("worker".into(), children),
    ])));
    let mut document = configuration();
    document["run"]["max_depth"] = 3.into();
    document["run"]["max_concurrent_children"] = max_children.into();
    document["endpoint"]["worker"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["worker"]["model"] = "worker".into();
    document["profile"]["worker"] = json!({"reply":["worker"],"compaction":["compaction"]});
    let (kernel, store) = kernel_configured(root, provider, document, vec![], |_| {}).await;
    let host = controlled(kernel, store);
    host.adapter.activity_enabled.store(true, Ordering::SeqCst);
    host.adapter
        .conversations_enabled
        .store(true, Ordering::SeqCst);
    Delegation {
        host,
        parent,
        child,
    }
}
fn delegation(to: &str) -> Result<enco_kernel::Completion, Failure> {
    calls(vec![call(
        "session_send",
        &json!({"to":to,"text":"research","profile":"worker"}).to_string(),
    )])
}
async fn begin(s: &mut Delegation, settings: ConnectionSettings) -> (Channel, Prompt, Prompt) {
    let channel = start_with(&s.host.kernel, &s.host.adapter, settings);
    receive(&mut s.host.polling).await;
    s.host
        .updates
        .send(vec![update(1, Action::Message("delegate".into()))])
        .await
        .unwrap();
    receive(&mut s.host.polling).await;
    let parent = receive(&mut s.parent).await;
    let draft = receive(&mut s.host.activity).await;
    assert!(draft.replies_here);
    draft.result.send(Ok(None)).unwrap();
    assert_eq!(receive(&mut s.host.presence).await, "chat");
    parent.respond(delegation("research"));
    let child = receive(&mut s.child).await;
    let parent = receive(&mut s.parent).await;
    (channel, parent, child)
}
async fn opened(s: &mut Delegation) {
    let opened = receive(&mut s.host.opening).await;
    assert_eq!(opened.beside, "chat");
    assert_eq!(opened.session, ["main", "research"]);
    opened.result.send(Ok(Some("child-topic".into()))).unwrap();
    let card = receive(&mut s.host.activity).await;
    assert_eq!(card.target, "child-topic");
    assert!(!card.replies_here);
    assert!(matches!(card.update, ChannelActivityUpdate::View(_)));
    card.result.send(Ok(Some("child-card".into()))).unwrap();
}

#[tokio::test]
async fn child_views_leave_epilogues_takeover_reuses_the_topic_and_archive_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    child.respond(reply("\n  found three papers  \nsecond line"));
    let epilogue = receive(&mut s.host.activity).await;
    assert_eq!(epilogue.target, "child-topic");
    assert!(!epilogue.replies_here);
    assert_eq!(epilogue.shown.as_deref(), Some("child-card"));
    let ChannelActivityUpdate::Epilogue(view) = &epilogue.update else {
        panic!("epilogue expected")
    };
    assert_eq!(view.rounds, 1);
    assert_eq!(view.end, RunEnd::Completed);
    assert!(!view.unsent);
    assert_eq!(view.excerpt, "found three papers");
    epilogue.result.send(Ok(None)).unwrap();
    // A presence goes out no later than the card it accompanies; the child's topic is only for
    // display.
    assert!(s.host.presence.try_recv().is_err());
    let mut takeover = update(2, Action::Message("new owner question".into()));
    takeover.input.conversation = "child-topic".into();
    takeover.input.conversation_name = Some(enco_kernel::ChannelConversationName::Fetch(
        "already-bound".into(),
    ));
    s.host.updates.send(vec![takeover]).await.unwrap();
    receive(&mut s.host.polling).await;
    assert!(s.host.naming.try_recv().is_err());
    let owner = receive(&mut s.child).await;
    let draft = receive(&mut s.host.activity).await;
    assert_eq!(draft.target, "child-topic");
    assert!(draft.replies_here);
    draft.result.send(Ok(None)).unwrap();
    assert_eq!(receive(&mut s.host.presence).await, "child-topic");
    owner.respond(reply("owner answer"));
    let answer = receive(&mut s.host.sending).await;
    assert_eq!(answer.target, "child-topic");
    answer.result.send(Ok(())).unwrap();
    parent.respond(delegation("research"));
    let again = receive(&mut s.child).await;
    let _parent = receive(&mut s.parent).await;
    let background = receive(&mut s.host.activity).await;
    assert_eq!(background.target, "child-topic");
    assert!(!background.replies_here);
    background
        .result
        .send(Ok(Some("second-card".into())))
        .unwrap();
    again.respond(reply("second task finished"));
    let epilogue = receive(&mut s.host.activity).await;
    assert_eq!(epilogue.shown.as_deref(), Some("second-card"));
    assert!(matches!(
        epilogue.update,
        ChannelActivityUpdate::Epilogue(_)
    ));
    epilogue.result.send(Ok(None)).unwrap();
    let child = s.host.kernel.find_session("main/research").await.unwrap();
    s.host
        .kernel
        .archive(child.id, EventSource::Cli)
        .await
        .unwrap();
    let close = receive(&mut s.host.closing).await;
    assert_eq!(close.target, "child-topic");
    close.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn retry_after_preserves_conversation_creation_and_fast_run_epilogue_until_they_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    let open = receive(&mut s.host.opening).await;
    tokio::time::pause();
    open.result.send(Err(limited())).unwrap();
    child.respond(reply("fast child reply"));
    tokio::time::advance(Duration::from_secs(9)).await;
    let open = receive(&mut s.host.opening).await;
    open.result.send(Ok(Some("child-topic".into()))).unwrap();
    let epilogue = receive(&mut s.host.activity).await;
    assert!(epilogue.shown.is_none());
    assert!(
        matches!(&epilogue.update, ChannelActivityUpdate::Epilogue(view) if view.excerpt == "fast child reply")
    );
    epilogue.result.send(Err(limited())).unwrap();
    tokio::time::advance(Duration::from_secs(9)).await;
    let epilogue = receive(&mut s.host.activity).await;
    assert!(!epilogue.replies_here);
    assert!(matches!(
        epilogue.update,
        ChannelActivityUpdate::Epilogue(_)
    ));
    epilogue.result.send(Ok(Some("epilogue".into()))).unwrap();
    tokio::time::resume();
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn terminal_or_unknown_creation_is_not_repeated_after_restart() {
    for unknown in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = delegated(dir.path(), 4).await;
        let (channel, _parent, child) = begin(&mut s, settings()).await;
        let mut fault = lost_response();
        fault.unknown = unknown;
        fault.failure.retryable = unknown;
        if !unknown {
            fault.failure.code = "bad_request".into();
            fault.failure.message = "the protocol cannot open a conversation here".into();
        }
        receive(&mut s.host.opening)
            .await
            .result
            .send(Err(fault))
            .unwrap();
        // A committed protocol checkpoint is a barrier after the in-flight creation is drained.
        channel.shutdown().await.unwrap();
        let channel = start(&s.host.kernel, &s.host.adapter);
        receive(&mut s.host.polling).await;
        child.respond(reply("not displayed"));
        let session = s.host.kernel.find_session("main/research").await.unwrap();
        let mut entries = s.host.kernel.subscribe(session.id).await.unwrap();
        if !s
            .host
            .kernel
            .log(session.id, None)
            .await
            .unwrap()
            .iter()
            .any(|entry| matches!(entry.body, EntryBody::RunEnded { .. }))
        {
            finish(&mut entries).await;
        }
        channel.shutdown().await.unwrap();
        assert!(s.host.opening.try_recv().is_err());
        assert_eq!(
            s.host.adapter.conversation_attempts.load(Ordering::SeqCst),
            1
        );
        assert!(s.host.activity.try_recv().is_err());
        s.host.kernel.shutdown().await.unwrap();
    }
}

/// Wait until the connection's committed state satisfies `ready`: a barrier before asserting
/// that nothing was shown, never itself the assertion.
async fn state_where(host: &Harness, ready: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state = host
                .kernel
                .connection("controlled:bot")
                .await
                .unwrap()
                .unwrap();
            if ready(&state) {
                return state;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

async fn completed_child(s: &Delegation, prompt: Prompt) -> (SessionId, LogPos) {
    let session = s
        .host
        .kernel
        .find_session("main/research")
        .await
        .unwrap()
        .id;
    let mut entries = s.host.kernel.subscribe(session).await.unwrap();
    prompt.respond(reply("child finished"));
    let log = finish(&mut entries).await;
    (session, log.last().unwrap().pos)
}

/// Wait until the connection has gone through `session`'s Log up to `pos`.
async fn projected(host: &Harness, session: SessionId, pos: LogPos) {
    state_where(host, |state| {
        serde_json::from_value::<Option<LogPos>>(
            state["outbound"][session.to_string()]["processed"].clone(),
        )
        .unwrap()
            >= Some(pos)
    })
    .await;
}

#[tokio::test]
async fn disabled_or_unsupported_children_never_get_a_card_or_another_creation_attempt() {
    for disabled in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = delegated(dir.path(), 4).await;
        s.host
            .adapter
            .conversations_enabled
            .store(false, Ordering::SeqCst);
        let mut policy = settings();
        policy.show_children = !disabled;
        let (channel, _parent, child) = begin(&mut s, policy).await;
        let (session, ended) = completed_child(&s, child).await;
        projected(&s.host, session, ended).await;
        channel.shutdown().await.unwrap();
        assert!(s.host.opening.try_recv().is_err());
        assert!(s.host.activity.try_recv().is_err());
        assert_eq!(
            s.host.adapter.conversation_attempts.load(Ordering::SeqCst),
            u32::from(!disabled)
        );
        s.host.kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn restart_with_children_disabled_keeps_the_existing_card_epilogue_and_route() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    channel.shutdown().await.unwrap();
    let mut policy = settings();
    policy.show_children = false;
    let channel = start_with(&s.host.kernel, &s.host.adapter, policy);
    receive(&mut s.host.polling).await;
    completed_child(&s, child).await;
    let epilogue = loop {
        let activity = receive(&mut s.host.activity).await;
        if matches!(activity.update, ChannelActivityUpdate::View(_)) {
            activity.result.send(Ok(None)).unwrap();
        } else {
            break activity;
        }
    };
    assert_eq!(epilogue.target, "child-topic");
    assert_eq!(epilogue.shown.as_deref(), Some("child-card"));
    assert!(matches!(
        epilogue.update,
        ChannelActivityUpdate::Epilogue(_)
    ));
    epilogue.result.send(Ok(None)).unwrap();
    let mut input = update(2, Action::Message("still yours".into()));
    input.input.conversation = "child-topic".into();
    s.host.updates.send(vec![input]).await.unwrap();
    receive(&mut s.host.polling).await;
    let prompt = receive(&mut s.child).await;
    assert!(prompt.contains("still yours"));
    let draft = receive(&mut s.host.activity).await;
    assert_eq!(draft.target, "child-topic");
    draft.result.send(Ok(None)).unwrap();
    prompt.respond(reply("answered here"));
    let answer = receive(&mut s.host.sending).await;
    assert_eq!(answer.target, "child-topic");
    answer.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_gone_child_topic_is_neither_shown_nor_opened_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    receive(&mut s.host.opening)
        .await
        .result
        .send(Ok(Some("child-topic".into())))
        .unwrap();
    let card = receive(&mut s.host.activity).await;
    let mut gone = lost_response();
    gone.unknown = false;
    gone.failure.code = code::CHANNEL_CONVERSATION_GONE.into();
    gone.failure.retryable = false;
    card.result.send(Err(gone)).unwrap();
    state_where(&s.host, |state| state["chats"].get("child-topic").is_none()).await;
    let (session, ended) = completed_child(&s, child).await;
    projected(&s.host, session, ended).await;
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    assert!(s.host.activity.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_opening_leaves_the_parents_conversation_reachable() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, parent, _child) = begin(&mut s, settings()).await;
    let mut gone = lost_response();
    gone.unknown = false;
    gone.failure.code = code::CHANNEL_CONVERSATION_GONE.into();
    gone.failure.retryable = false;
    receive(&mut s.host.opening)
        .await
        .result
        .send(Err(gone))
        .unwrap();
    // Whatever the protocol says about opening beside it, the parent's replies still go there.
    parent.respond(reply("still here"));
    let answer = receive(&mut s.host.sending).await;
    assert_eq!(answer.target, "chat");
    answer.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn descendants_open_beside_their_parents_display_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    child.respond(delegation("nested"));
    let _first = receive(&mut s.child).await;
    let _second = receive(&mut s.child).await;
    let open = receive(&mut s.host.opening).await;
    assert_eq!(open.beside, "child-topic");
    assert_eq!(open.session, ["main", "research", "nested"]);
    open.result.send(Ok(Some("nested-topic".into()))).unwrap();
    let card = receive(&mut s.host.activity).await;
    assert_eq!(card.target, "nested-topic");
    assert!(!card.replies_here);
    card.result.send(Ok(Some("nested-card".into()))).unwrap();
    channel.shutdown().await.unwrap();
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_rebound_child_topic_shows_no_more_child_views_and_survives_its_archival() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    let (session, _) = completed_child(&s, child).await;
    receive(&mut s.host.activity)
        .await
        .result
        .send(Ok(None))
        .unwrap();
    let mut rebind = update(2, Action::SelectSession("main".into()));
    rebind.input.conversation = "child-topic".into();
    s.host.updates.send(vec![rebind]).await.unwrap();
    receive(&mut s.host.polling).await;
    receive(&mut s.host.sending)
        .await
        .result
        .send(Ok(()))
        .unwrap();
    parent.respond(delegation("research"));
    let child = receive(&mut s.child).await;
    let _parent = receive(&mut s.parent).await;
    let (_, ended) = completed_child(&s, child).await;
    projected(&s.host, session, ended).await;
    s.host
        .kernel
        .archive(session, EventSource::Cli)
        .await
        .unwrap();
    // The connection lets go of an archived child it has nothing left to show for.
    state_where(&s.host, |state| {
        state["outbound"].get(session.to_string()).is_none()
    })
    .await;
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    assert!(s.host.activity.try_recv().is_err());
    // The topic is the owner's conversation with main now; archiving the child leaves it alone.
    assert!(s.host.closing.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn archival_close_retries_known_refusal_then_forgets_binding_even_when_closing_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    let (session, _) = completed_child(&s, child).await;
    receive(&mut s.host.activity)
        .await
        .result
        .send(Ok(None))
        .unwrap();
    s.host
        .kernel
        .archive(session, EventSource::Cli)
        .await
        .unwrap();
    let close = receive(&mut s.host.closing).await;
    tokio::time::pause();
    close.result.send(Err(limited())).unwrap();
    tokio::time::advance(Duration::from_secs(9)).await;
    let close = receive(&mut s.host.closing).await;
    let mut refused = lost_response();
    refused.unknown = false;
    refused.failure.retryable = false;
    close.result.send(Err(refused)).unwrap();
    tokio::time::resume();
    state_where(&s.host, |state| state["chats"].get("child-topic").is_none()).await;
    let mut input = update(2, Action::Message("new topic".into()));
    input.input.conversation = "child-topic".into();
    input.input.realm = Some("chat".into());
    input.input.conversation_name = Some(enco_kernel::ChannelConversationName::Name(
        "fresh root".into(),
    ));
    s.host.updates.send(vec![input]).await.unwrap();
    receive(&mut s.host.polling).await;
    let root = receive(&mut s.parent).await;
    let draft = receive(&mut s.host.activity).await;
    assert!(draft.replies_here);
    draft.result.send(Ok(None)).unwrap();
    root.respond(reply("fresh reply"));
    let answer = receive(&mut s.host.sending).await;
    assert_eq!(answer.target, "child-topic");
    answer.result.send(Ok(())).unwrap();
    let fresh = s.host.kernel.find_session("fresh-root").await.unwrap();
    assert!(fresh.parent.is_none());
    assert_ne!(fresh.id, session);
    channel.shutdown().await.unwrap();
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn cli_destination_does_not_open_child_conversations_on_a_following_connection() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let channel = start(&s.host.kernel, &s.host.adapter);
    receive(&mut s.host.polling).await;
    s.host
        .updates
        .send(vec![update(1, Action::Message("seed".into()))])
        .await
        .unwrap();
    receive(&mut s.host.polling).await;
    let seed = receive(&mut s.parent).await;
    receive(&mut s.host.activity)
        .await
        .result
        .send(Ok(None))
        .unwrap();
    seed.respond(reply("seeded"));
    receive(&mut s.host.sending)
        .await
        .result
        .send(Ok(()))
        .unwrap();
    let parent = s.host.kernel.find_session("main").await.unwrap().id;
    s.host
        .kernel
        .submit(parent, EventId::new(), "from CLI".into(), vec![])
        .await
        .unwrap();
    receive(&mut s.parent).await.respond(delegation("research"));
    let child = receive(&mut s.child).await;
    let _parent = receive(&mut s.parent).await;
    let (child, ended) = completed_child(&s, child).await;
    projected(&s.host, child, ended).await;
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    assert!(s.host.activity.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_created_child_waiting_for_a_run_slot_does_not_open_a_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 1).await;
    let (channel, parent, _child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    parent.respond(delegation("queued"));
    let _parent = receive(&mut s.parent).await;
    let queued = s.host.kernel.find_session("main/queued").await.unwrap().id;
    state_where(&s.host, |state| {
        state["outbound"].get(queued.to_string()).is_some()
    })
    .await;
    let status = s.host.kernel.status().await.unwrap();
    assert!(
        status
            .sessions
            .iter()
            .any(|session| session.session.id == queued && session.waiting == Some(1))
    );
    assert!(
        !s.host
            .kernel
            .log(queued, None)
            .await
            .unwrap()
            .iter()
            .any(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
    );
    channel.shutdown().await.unwrap();
    assert!(s.host.opening.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

async fn stop_in_child_topic(
    s: &mut Delegation,
    entries: &mut tokio::sync::broadcast::Receiver<Entry>,
    offset: u32,
    command: Action,
    end: RunEnd,
) {
    let mut input = update(offset, command);
    input.input.conversation = "child-topic".into();
    s.host.updates.send(vec![input]).await.unwrap();
    receive(&mut s.host.polling).await;
    let log = finish(entries).await;
    assert!(
        matches!(&log.last().unwrap().body, EntryBody::RunEnded { end: actual, .. } if *actual == end)
    );
    receive(&mut s.host.sending)
        .await
        .result
        .send(Ok(()))
        .unwrap();
    let epilogue = receive(&mut s.host.activity).await;
    assert!(matches!(&epilogue.update, ChannelActivityUpdate::Epilogue(view) if view.end == end));
    epilogue.result.send(Ok(None)).unwrap();
}

#[tokio::test]
async fn child_conversation_cancel_and_unnamed_halt_target_the_bound_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, parent, _child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    let child = s
        .host
        .kernel
        .find_session("main/research")
        .await
        .unwrap()
        .id;
    let mut entries = s.host.kernel.subscribe(child).await.unwrap();
    stop_in_child_topic(&mut s, &mut entries, 2, Action::Cancel, RunEnd::Cancelled).await;
    parent.respond(delegation("research"));
    let _child = receive(&mut s.child).await;
    let _parent = receive(&mut s.parent).await;
    let card = receive(&mut s.host.activity).await;
    card.result.send(Ok(Some("next-card".into()))).unwrap();
    stop_in_child_topic(&mut s, &mut entries, 3, Action::Halt(None), RunEnd::Halted).await;
    channel.shutdown().await.unwrap();
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn takeover_during_a_run_removes_its_background_card_before_showing_a_waiting_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    let mut input = update(2, Action::Message("owner takes over".into()));
    input.input.conversation = "child-topic".into();
    s.host.updates.send(vec![input]).await.unwrap();
    receive(&mut s.host.polling).await;
    child.respond(calls(vec![call(
        "message_send",
        r#"{"text":"before takeover"}"#,
    )]));
    let owner = receive(&mut s.child).await;
    assert!(owner.contains("owner takes over"));
    let remove = receive(&mut s.host.activity).await;
    assert_eq!(remove.target, "child-topic");
    assert_eq!(remove.shown.as_deref(), Some("child-card"));
    assert!(!remove.replies_here);
    assert!(matches!(remove.update, ChannelActivityUpdate::Remove));
    remove.result.send(Ok(None)).unwrap();
    let draft = receive(&mut s.host.activity).await;
    assert!(draft.replies_here);
    assert!(draft.shown.is_none());
    draft.result.send(Ok(None)).unwrap();
    assert_eq!(receive(&mut s.host.presence).await, "child-topic");
    let child_id = s
        .host
        .kernel
        .find_session("main/research")
        .await
        .unwrap()
        .id;
    let mut entries = s.host.kernel.subscribe(child_id).await.unwrap();
    owner.respond(reply("owner answer"));
    finish(&mut entries).await;
    let answer = receive(&mut s.host.sending).await;
    assert_eq!(answer.target, "child-topic");
    assert_eq!(answer.text, "owner answer");
    answer.result.send(Ok(())).unwrap();
    let parent = s.host.kernel.find_session("main").await.unwrap().id;
    let reports: Vec<_> = s
        .host
        .store
        .pending(parent)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.body {
            EventBody::SessionMessage(outgoing) => Some(outgoing.text),
            _ => None,
        })
        .collect();
    assert_eq!(reports, ["before takeover"]);
    channel.shutdown().await.unwrap();
    assert!(s.host.activity.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn rebinding_during_a_run_removes_the_old_card_and_never_writes_its_epilogue_to_the_new_session()
 {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, _parent, child) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    let mut input = update(2, Action::SelectSession("main".into()));
    input.input.conversation = "child-topic".into();
    s.host.updates.send(vec![input]).await.unwrap();
    receive(&mut s.host.polling).await;
    receive(&mut s.host.sending)
        .await
        .result
        .send(Ok(()))
        .unwrap();
    let remove = receive(&mut s.host.activity).await;
    assert!(matches!(remove.update, ChannelActivityUpdate::Remove));
    assert_eq!(remove.target, "child-topic");
    remove.result.send(Ok(None)).unwrap();
    let (session, ended) = completed_child(&s, child).await;
    projected(&s.host, session, ended).await;
    channel.shutdown().await.unwrap();
    assert!(s.host.activity.try_recv().is_err());
    assert!(s.host.opening.try_recv().is_err());
    s.host.kernel.shutdown().await.unwrap();
}
