use super::*;
use enco_kernel::ChannelConversationName;

/// The owner's message in a topic of the private chat's realm.
fn topic(offset: u32, conversation: &str, name: Option<ChannelConversationName>) -> Update {
    let mut update = update(offset, Action::Message("hello".into()));
    update.input.conversation = conversation.into();
    update.input.realm = Some("chat".into());
    update.input.conversation_name = name;
    update
}

/// How many owner messages the Session named `name` has taken in.
async fn messages(kernel: &Kernel, name: &str) -> usize {
    let session = kernel.find_session(name).await.unwrap();
    kernel
        .log(session.id, None)
        .await
        .unwrap()
        .iter()
        .filter(|entry| {
            matches!(&entry.body, EntryBody::EventConsumed { event } if matches!(event.body, EventBody::UserMessage { .. }))
        })
        .count()
}

#[tokio::test]
async fn private_topics_get_roots_named_by_titles_or_ids_and_keep_their_routes() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new((0..8).map(|_| reply("answer")).collect());
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut naming,
        ..
    } = harness(dir.path(), provider).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    let name = |title: &str| Some(ChannelConversationName::Name(title.into()));
    let plain = update(1, Action::Message("hello".into()));
    for (input, expected) in [
        (plain, "main"),
        (topic(2, "topic-a", name("main")), "main-2"),
        (
            topic(3, "topic-b", name(" work / research ")),
            "work-research",
        ),
        (
            topic(4, "topic-c", name(" work / research ")),
            "work-research-2",
        ),
        (topic(5, "topic-d", name("   ")), "controlled:topic-d"),
        // A topic whose title is unknown, such as one older than the connection, is still a root.
        (topic(6, "topic-e", None), "controlled:topic-e"),
    ] {
        let conversation = input.input.conversation.clone();
        updates.send(vec![input]).await.unwrap();
        receive(&mut polling).await;
        let answer = receive(&mut sending).await;
        assert_eq!(answer.target, conversation);
        answer.result.send(Ok(())).unwrap();
        let session = kernel.find_session(expected).await.unwrap();
        assert!(session.parent.is_none());
        assert_eq!(session.profile.as_str(), "default");
        assert_eq!(messages(&kernel, expected).await, 1);
    }
    // Neither changed titles nor locators cause a new name or I/O on a live route.
    for (offset, name) in [
        (7, name("renamed")),
        (8, Some(ChannelConversationName::Fetch("unneeded".into()))),
    ] {
        updates
            .send(vec![topic(offset, "topic-b", name)])
            .await
            .unwrap();
        receive(&mut polling).await;
        receive(&mut sending).await.result.send(Ok(())).unwrap();
    }
    assert!(naming.try_recv().is_err());
    assert!(kernel.find_session("renamed").await.is_err());
    assert_eq!(messages(&kernel, "work-research").await, 3);
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn private_realms_fetch_names_only_for_new_messages_and_archived_routes() {
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut naming,
        mut closing,
        ..
    } = harness(
        dir.path(),
        ScriptedProvider::new(vec![reply("first"), reply("second"), reply("third")]),
    )
    .await;
    let mut policy = settings();
    policy.private_realms.insert("guild".into());
    let channel = start_with(&kernel, &adapter, policy);
    receive(&mut polling).await;
    let private = |offset, action| {
        let mut value = update(offset, action);
        value.input.direct = false;
        value.input.addressed = false;
        value.input.realm = Some("guild".into());
        value.input.conversation_name = Some(ChannelConversationName::Fetch("channel-name".into()));
        value
    };
    updates
        .send(vec![private(1, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(naming.try_recv().is_err());
    updates
        .send(vec![private(2, Action::Message("owner".into()))])
        .await
        .unwrap();
    let name = receive(&mut naming).await;
    assert_eq!(name.locator, "channel-name");
    name.result
        .send(Ok(fetched(Ok("private / research".into()))))
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    let original = kernel.find_session("private-research").await.unwrap();
    assert_eq!(original.profile.as_str(), "default");
    updates
        .send(vec![private(3, Action::Message("again".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert!(naming.try_recv().is_err());
    kernel.archive(original.id, EventSource::Cli).await.unwrap();
    updates
        .send(vec![private(4, Action::Message("new".into()))])
        .await
        .unwrap();
    receive(&mut naming)
        .await
        .result
        .send(Ok(fetched(Err("unavailable"))))
        .unwrap();
    receive(&mut polling).await;
    for _ in 0..2 {
        receive(&mut sending).await.result.send(Ok(())).unwrap();
    }
    let replacement = kernel.find_session("controlled:chat").await.unwrap();
    assert_eq!(replacement.profile.as_str(), "default");
    assert_ne!(replacement.id, original.id);
    assert!(closing.try_recv().is_err());
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn public_forum_conversations_use_group_profiles_without_fetching_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut document = configuration();
    document["profile"]["group"] = json!({"reply":["reply"],"compaction":["compaction"],"tools":[],"owner_context":false,"requires_lifeline":false});
    let (kernel, store) = kernel_configured(
        dir.path(),
        ScriptedProvider::new(vec![reply("one"), reply("two")]),
        document,
        vec![],
        |_| {},
    )
    .await;
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut naming,
        ..
    } = controlled(kernel, store);
    let mut policy = settings();
    policy.group_profile = Some("group".parse().unwrap());
    let channel = start_with(&kernel, &adapter, policy);
    receive(&mut polling).await;
    for (offset, target) in [(1, "forum/1"), (2, "forum/2")] {
        let mut value = topic(
            offset,
            target,
            Some(ChannelConversationName::Fetch("public-title".into())),
        );
        value.input.direct = false;
        value.input.realm = Some("public-guild".into());
        updates.send(vec![value]).await.unwrap();
        receive(&mut polling).await;
        let answer = receive(&mut sending).await;
        assert_eq!(answer.target, target);
        answer.result.send(Ok(())).unwrap();
    }
    let sessions = kernel.sessions().await.unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions
            .iter()
            .all(|session| session.profile.as_str() == "group")
    );
    assert!(naming.try_recv().is_err());
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
