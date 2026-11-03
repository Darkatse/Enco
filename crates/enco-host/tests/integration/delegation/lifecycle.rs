use super::*;

#[tokio::test]
async fn halt_consumes_prior_inputs_stops_descendants_and_allows_later_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |document| {
        document["run"]["max_depth"] = 2.into()
    })
    .await;
    let (parent, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    h.next("worker")
        .await
        .respond(calls(vec![send("review", "nested task", Some("second"))]));
    let held_child = h.next("worker").await;
    let held_grandchild = h.next("second").await;
    let child = h.child("main/research").await;
    let grandchild = h.child("main/research/review").await;
    h.kernel
        .submit(
            child.id,
            EventId::new(),
            "do not answer this".into(),
            vec![],
        )
        .await
        .unwrap();
    let halted = h.kernel.halt(parent, EventSource::Cli).await.unwrap();
    let ids: Vec<_> = halted.sessions.iter().map(|session| session.id).collect();
    assert_eq!(ids, [parent, child.id, grandchild.id]);
    assert!(halted.stopped.is_empty());
    for session in [parent, child.id, grandchild.id] {
        assert!(h.store.pending(session).await.unwrap().is_empty());
        assert!(
            !h.kernel
                .status()
                .await
                .unwrap()
                .sessions
                .iter()
                .find(|status| status.session.id == session)
                .unwrap()
                .running
        );
    }
    for session in [child.id, grandchild.id] {
        let entries = h.kernel.log(session, None).await.unwrap();
        assert!(entries.iter().any(|entry| matches!(
            entry.body,
            EntryBody::RunEnded {
                end: RunEnd::Halted,
                ..
            }
        )));
        assert!(h.store.child_state(session).await.unwrap().halted.is_some());
    }
    drop((held_child, held_grandchild));
    h.kernel
        .submit(parent, EventId::new(), "continue".into(), vec![])
        .await
        .unwrap();
    h.next("fixture").await.respond(reply("ready"));
    // The subscription also contains the halt's EventConsumed before the new Run.
    finish(&mut rx).await;
    // A halt wakes nobody: the parent's later Run consumed nothing from its children.
    assert!(!from_child(&h.kernel.log(parent, None).await.unwrap()));
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn parent_halt_and_archive_tools_wait_for_quiescence_and_archive_releases_names() {
    for archive in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut h = Harness::new(dir.path(), |_| {}).await;
        let (parent, mut rx) = start(&h).await;
        h.next("fixture")
            .await
            .respond(calls(vec![send("research", "task", Some("worker"))]));
        let held = h.next("worker").await;
        h.next("fixture").await.respond(reply("delegated"));
        finish(&mut rx).await;
        let child = h.child("main/research").await;
        let mut child_rx = h.kernel.subscribe(child.id).await.unwrap();
        h.kernel
            .submit(parent, EventId::new(), "stop the child".into(), vec![])
            .await
            .unwrap();
        h.next("fixture").await.respond(calls(vec![call(
            if archive {
                "session_archive"
            } else {
                "session_halt"
            },
            r#"{"session":"research"}"#,
        )]));
        let parent_after = h.next("fixture").await;
        let stored = h.store.session(child.id).await.unwrap().unwrap();
        assert_eq!(stored.archived_at.is_some(), archive);
        assert!(h.store.pending(child.id).await.unwrap().is_empty());
        assert!(
            h.latest("main")
                .children
                .iter()
                .all(|status| !status.state.awaiting)
        );
        if archive {
            assert!(h.latest("main").children.is_empty());
            // The archived child has stopped: a subscription to it ends at once.
            assert!(matches!(
                h.kernel.subscribe(child.id).await.unwrap().try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Closed)
            ));
            let log = h.kernel.log(child.id, None).await.unwrap();
            assert!(log.iter().any(|entry| matches!(
                entry.body,
                EntryBody::RunEnded {
                    end: RunEnd::Halted,
                    ..
                }
            )));
            while child_rx.recv().await.is_ok() {}
            assert_eq!(
                h.kernel
                    .submit(child.id, EventId::new(), "late".into(), vec![])
                    .await
                    .unwrap(),
                Accepted::Archived
            );
            parent_after.respond(calls(vec![send("research", "new task", Some("worker"))]));
            let new_worker = h.next("worker").await;
            let new_child = h.child("main/research").await;
            assert_ne!(new_child.id, child.id);
            h.next("fixture").await.respond(reply("new child"));
            finish(&mut rx).await;
            new_worker.respond(reply("new result"));
            h.next("fixture")
                .await
                .respond(crate::silence::skip("accepted"));
            finish(&mut rx).await;
        } else {
            let status = &h.latest("main").children[0];
            assert!(status.state.halted.is_some());
            assert!(!status.state.replies_to_owner);
            parent_after.respond(reply("halted"));
            finish(&mut rx).await;
        }
        drop(held);
        h.kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancelling_a_parent_run_does_not_cancel_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (parent, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let child_prompt = h.next("worker").await;
    let parent_prompt = h.next("fixture").await;
    assert!(h.kernel.cancel(parent).unwrap());
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Cancelled,
            ..
        }
    ));
    let child = h.child("main/research").await;
    assert!(
        h.kernel
            .status()
            .await
            .unwrap()
            .sessions
            .iter()
            .find(|status| status.session.id == child.id)
            .unwrap()
            .running
    );
    drop(parent_prompt);
    child_prompt.respond(reply("still completed"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("noted"));
    assert!(received(&finish(&mut rx).await, "still completed"));
    h.kernel.shutdown().await.unwrap();
}

/// A read-only call that runs until its batch is cancelled, so the batch settles after a halt.
struct UntilCancelled(tokio::sync::Notify);

#[async_trait]
impl Tool for UntilCancelled {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "until-cancelled".into(),
            version: "test".into(),
        }
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "wait".into(),
            description: "Test call that runs until it is cancelled.".into(),
            input_schema: InputSchema::empty(),
            effect: Effect::ReadOnly,
        }
    }

    async fn call(
        &self,
        ctx: CallContext,
        _: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome {
        self.0.notify_one();
        ctx.cancel.cancelled().await;
        Outcome::Ok {
            images: Vec::new(),
            outgoing: None,
            value: serde_json::json!("stopped"),
        }
    }
}

#[tokio::test]
async fn a_halt_fails_a_delegation_that_settles_after_it_and_creates_no_child() {
    let dir = tempfile::tempdir().unwrap();
    let wait = Arc::new(UntilCancelled(tokio::sync::Notify::new()));
    let mut h = Harness::with_tools(dir.path(), vec![wait.clone()], |_| {}).await;
    let (parent, _rx) = start(&h).await;
    h.next("fixture").await.respond(calls(vec![
        call("wait", "{}"),
        send("research", "task", Some("worker")),
    ]));
    wait.0.notified().await;
    h.kernel.halt(parent, EventSource::Cli).await.unwrap();
    let entries = h.kernel.log(parent, None).await.unwrap();
    assert!(entries.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::ToolCallSettled {
            outcome: Settlement::Failed { failure },
            delegated: None,
            ..
        } if failure.code == code::TOOL_FAILED
    )));
    assert!(entries.iter().all(|entry| !matches!(
        entry.body,
        EntryBody::ToolCallSettled {
            delegated: Some(_),
            ..
        }
    )));
    assert!(matches!(
        h.kernel.find_session("main/research").await,
        Err(KernelError::Lookup(SessionLookupError::Unknown(_)))
    ));
    assert!(entries.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RunEnded {
            end: RunEnd::Halted,
            ..
        }
    )));
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_owner_backtrack_in_a_child_tells_its_parent_and_keeps_the_answer_for_the_owner() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (parent, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let interrupted = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    let child = h.child("main/research").await;
    let task = h
        .kernel
        .log(child.id, None)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| matches!(entry.body, EntryBody::EventConsumed { .. }))
        .unwrap()
        .pos;
    h.kernel
        .backtrack(child.id, EventId::new(), Some(task))
        .await
        .unwrap();
    drop(interrupted);
    // The parent learns how the Run it waited on ended, and that the owner now has the child.
    let parent_prompt = h.next("fixture").await;
    let state = &h.latest("main").children[0].state;
    assert!(state.replies_to_owner && !state.awaiting);
    parent_prompt.respond(crate::silence::skip("the owner took it over"));
    let entries = finish(&mut rx).await;
    assert!(entries.iter().any(|entry| matches!(
        &entry.body,
        EntryBody::EventConsumed {
            event: Event {
                body: EventBody::ChildRunEnded {
                    end: RunEnd::Backtracked
                },
                ..
            }
        }
    )));
    // The child answers the task again, now for the owner.
    let mut child_rx = h.kernel.subscribe(child.id).await.unwrap();
    h.next("worker").await.respond(reply("for the owner"));
    finish(&mut child_rx).await;
    assert!(
        !reports(&h, parent)
            .await
            .contains(&"for the owner".to_owned())
    );
    h.kernel.shutdown().await.unwrap();
}
