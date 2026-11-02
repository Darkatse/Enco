use super::*;

#[tokio::test]
async fn child_permissions_intersect_ancestors_and_depth_hides_delegation() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |document| {
        document["profile"]["default"]["tools"] =
            serde_json::json!(["session_send", "message_send", "fs_list", "reply_skip"]);
        document["profile"]["default"]["requires_lifeline"] = false.into();
        document["profile"]["default"]["owner_context"] = false.into();
    })
    .await;
    let (_, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let worker = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    let input = h.latest("main/research");
    assert!(!input.owner_context);
    let names: Vec<_> = input
        .tools
        .iter()
        .map(|(_, spec)| spec.name.as_str())
        .collect();
    assert!(names.contains(&"fs_list") && names.contains(&"message_send"));
    for unavailable in [
        "shell_exec",
        "session_send",
        "session_halt",
        "session_archive",
    ] {
        assert!(!names.contains(&unavailable));
    }
    assert!(
        input
            .lifeline
            .iter()
            .all(|id| input.tools.iter().any(|(tool, _)| tool == id))
    );
    assert!(input.profiles.is_empty());
    let forbidden = call(
        "session_send",
        r#"{"to":"nested","text":"task","profile":"worker"}"#,
    );
    let id = forbidden.id;
    worker.respond(calls(vec![forbidden]));
    h.next("worker")
        .await
        .respond(reply("done within permissions"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("accepted"));
    finish(&mut rx).await;
    let child = h.child("main/research").await;
    let entries = h.kernel.log(child.id, None).await.unwrap();
    assert!(entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::ToolCallSettled { call, outcome: Settlement::Failed { failure }, .. }
        if *call == id && failure.code == code::TOOL_UNAVAILABLE)));
    h.kernel.shutdown().await.unwrap();
}

async fn waiting(h: &Harness, name: &str, position: usize) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let status = h.kernel.status().await.unwrap();
            if status.sessions.iter().any(|status| {
                status.session.name.as_str() == name && status.waiting == Some(position)
            }) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn child_slots_queue_fifo_followups_keep_their_place_and_halts_need_no_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |document| {
        document["run"]["max_concurrent_children"] = 1.into()
    })
    .await;
    let (parent, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("first", "first task", Some("worker"))]));
    let first = h.next("worker").await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("second", "second task", Some("second"))]));
    let parent_next = h.next("fixture").await;
    waiting(&h, "main/second", 1).await;
    parent_next.respond(calls(vec![send("third", "third task", Some("third"))]));
    let parent_next = h.next("fixture").await;
    waiting(&h, "main/third", 2).await;
    parent_next.respond(calls(vec![send("second", "extra detail", None)]));
    let parent_next = h.next("fixture").await;
    waiting(&h, "main/second", 1).await;
    waiting(&h, "main/third", 2).await;
    parent_next.respond(reply("queued"));
    finish(&mut rx).await;
    first.respond(reply("first result"));
    let second = h.next("second").await;
    assert!(second.contains("second task") && second.contains("extra detail"));
    waiting(&h, "main/third", 1).await;
    let third = h.child("main/third").await;
    h.kernel.halt(third.id, EventSource::Cli).await.unwrap();
    assert!(h.store.pending(third.id).await.unwrap().is_empty());
    assert!(
        !h.kernel
            .log(third.id, None)
            .await
            .unwrap()
            .iter()
            .any(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
    );
    h.next("fixture")
        .await
        .respond(crate::silence::skip("first accepted"));
    finish(&mut rx).await;
    second.respond(reply("second result"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("second accepted"));
    finish(&mut rx).await;
    // The halted child never started a Run, before or after the slot freed.
    assert!(
        !h.kernel
            .log(third.id, None)
            .await
            .unwrap()
            .iter()
            .any(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
    );
    assert!(
        !h.kernel
            .status()
            .await
            .unwrap()
            .sessions
            .iter()
            .find(|status| status.session.id == parent)
            .unwrap()
            .running
    );
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn lowering_child_limit_does_not_preempt_runs_and_controls_later_admission() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |document| {
        document["run"]["max_concurrent_children"] = 2.into()
    })
    .await;
    let (_, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("first", "first task", Some("worker"))]));
    let first = h.next("worker").await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("second", "second task", Some("second"))]));
    let second = h.next("second").await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("third", "third task", Some("third"))]));
    let parent_next = h.next("fixture").await;
    waiting(&h, "main/third", 1).await;
    let mut document = h.kernel.registry().view().config.document.clone();
    document["run"]["max_concurrent_children"] = 1.into();
    submit(h.kernel.registry(), configuration_request(document))
        .await
        .unwrap();
    for name in ["main/first", "main/second"] {
        assert!(
            h.kernel
                .status()
                .await
                .unwrap()
                .sessions
                .iter()
                .find(|status| status.session.name.as_str() == name)
                .unwrap()
                .running
        );
    }
    parent_next.respond(reply("all delegated"));
    finish(&mut rx).await;
    first.respond(reply("first done"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("first accepted"));
    finish(&mut rx).await;
    // Still waiting: the lowered limit holds it while `second` runs.
    waiting(&h, "main/third", 1).await;
    second.respond(reply("second done"));
    let third = h.next("third").await;
    let parent = h.next("fixture").await;
    parent.respond(crate::silence::skip("second accepted"));
    finish(&mut rx).await;
    third.respond(reply("third done"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("third accepted"));
    finish(&mut rx).await;
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn nested_delegation_with_every_slot_taken_returns_the_slot_instead_of_deadlocking() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |document| {
        document["run"]["max_depth"] = 2.into();
        document["run"]["max_concurrent_children"] = 1.into();
    })
    .await;
    let (_, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let research = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    research.respond(calls(vec![send("review", "nested task", Some("second"))]));
    // The grandchild waits while its parent holds the only slot.
    waiting(&h, "main/research/review", 1).await;
    h.next("worker")
        .await
        .respond(reply("the review is on its way"));
    h.next("fixture")
        .await
        .respond(crate::silence::skip("noted"));
    finish(&mut rx).await;
    // The parent's Run ended and returned the slot, so the grandchild runs.
    h.next("second").await.respond(reply("review result"));
    let research = h.next("worker").await;
    assert!(research.contains("review result"));
    research.respond(reply("research result"));
    let main = h.next("fixture").await;
    assert!(main.contains("research result"));
    main.respond(reply("all done"));
    finish(&mut rx).await;
    h.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_facade_that_discloses_only_delegation_leaves_its_childs_tools_intact() {
    let dir = tempfile::tempdir().unwrap();
    // A facade's profile need not ask for the lifeline, so its composer may disclose only this.
    let mut h = Harness::new(dir.path(), |document| {
        document["profile"]["default"]["requires_lifeline"] = false.into();
    })
    .await;
    h.composer
        .facade
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (_, mut rx) = start(&h).await;
    let facade = h.next("fixture").await;
    let disclosed: Vec<_> = facade.request.tools.iter().map(|tool| &tool.name).collect();
    assert_eq!(disclosed, ["session_send"]);
    facade.respond(calls(vec![send("research", "task", Some("worker"))]));
    // Disclosure is the facade's focus, not a permission: its child still works with the shell.
    let worker = h.next("worker").await;
    assert!(
        worker
            .request
            .tools
            .iter()
            .any(|tool| tool.name == "shell_exec")
    );
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    drop(worker);
    h.kernel.shutdown().await.unwrap();
}
