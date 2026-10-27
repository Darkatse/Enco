use super::*;

fn group(offset: u32, sender: &str, addressed: bool, action: Action) -> Update {
    let mut value = update(offset, action);
    let input = &mut value.input;
    input.direct = false;
    input.addressed = addressed;
    input.sender = Some(sender.into());
    input.sender_name = Some(format!("{sender} display"));
    value
}

async fn group_harness(root: &Path, provider: Arc<ScriptedProvider>) -> Harness {
    let mut document = configuration();
    document["profile"]["group"] = json!({"reply": ["reply"], "compaction": ["compaction"], "tools": [], "owner_context": false, "requires_lifeline": false});
    document["profile"]["second"] = document["profile"]["group"].clone();
    let (kernel, store) = kernel_configured(root, provider, document, vec![], |_| {}).await;
    controlled(kernel, store)
}

#[tokio::test]
async fn groups_quote_bounded_context_and_keep_a_fresh_session_across_creation_profile_changes() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("owner answer"), reply("still group")]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut fetching,
        ..
    } = group_harness(dir.path(), provider).await;
    let private = kernel.open_session("controlled:chat").await.unwrap();
    let mut policy = settings();
    policy.group_profile = Some("group".into());
    policy.context = 2;
    let channel = start_with(&kernel, &adapter, policy.clone());
    receive(&mut polling).await;
    updates
        .send(vec![
            group(1, "owner", false, Action::Message("oldest".into())),
            group(2, "member", false, photo()),
            group(3, "member", true, Action::Message("background".into())),
            group(4, "owner", true, Action::Message("question".into())),
        ])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(4));
    assert!(fetching.try_recv().is_err());
    let answer = receive(&mut sending).await;
    assert_eq!(answer.text, "owner answer");
    answer.result.send(Ok(())).unwrap();
    let sessions = kernel.sessions().await.unwrap();
    let session = sessions
        .iter()
        .find(|session| session.id != private.id)
        .unwrap();
    assert_eq!(session.profile, "group");
    assert!(session.name.starts_with("controlled:chat"));
    assert_eq!(kernel.log(private.id, None).await.unwrap().len(), 0);
    let session_id = session.id;
    let log = kernel.log(session_id, None).await.unwrap();
    let event = log
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::EventConsumed { event } => Some(event),
            _ => None,
        })
        .unwrap();
    let EventBody::UserMessage { earlier, .. } = &event.body else {
        panic!("message expected")
    };
    assert_eq!(
        earlier
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        ["caption", "background"]
    );
    assert_eq!(earlier[0].attachments, ["photo"]);
    assert!(!earlier[0].owner);
    assert_eq!(earlier[0].sender.as_deref(), Some("member"));

    channel.shutdown().await.unwrap();

    // Changing the creation profile does not replace an already routed Session.
    policy.group_profile = Some("second".into());
    let channel = start_with(&kernel, &adapter, policy);
    receive(&mut polling).await;
    updates
        .send(vec![group(
            14,
            "owner",
            true,
            Action::Message("after restart".into()),
        )])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert_eq!(
        kernel
            .sessions()
            .await
            .unwrap()
            .iter()
            .find(|session| session.id == session_id)
            .unwrap()
            .profile,
        "group"
    );
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn only_new_owner_commands_open_groups_and_member_sources_survive_backtrack() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("member answer"), reply("back again")]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = group_harness(dir.path(), provider.clone()).await;
    let mut policy = settings();
    policy.group_profile = Some("group".into());
    let channel = start_with(&kernel, &adapter, policy);
    receive(&mut polling).await;
    let mut open = group(5, "owner", true, Action::Everyone(Some(true)));
    open.key = "open".into();
    updates
        .send(vec![
            open,
            group(6, "member", true, Action::Message("member question".into())),
        ])
        .await
        .unwrap();
    let receipt = receive(&mut sending).await;
    assert_eq!(
        receipt.text,
        "Everyone in this group can now ask the Agent."
    );
    receipt.result.send(Ok(())).unwrap();
    assert_eq!(receive(&mut polling).await, json!(6));
    let answer = receive(&mut sending).await;
    assert_eq!(answer.text, "member answer");
    answer.result.send(Ok(())).unwrap();
    let session_id = kernel.sessions().await.unwrap()[0].id;
    let log = kernel.log(session_id, None).await.unwrap();
    let member = log.iter().find_map(|entry| match &entry.body { EntryBody::EventConsumed { event } if matches!(&event.source, EventSource::Channel { message, .. } if message == "6") => Some(event), _ => None }).unwrap();
    assert!(
        matches!(&member.source, EventSource::Channel { owner: false, sender_name: Some(name), .. } if name == "member display")
    );
    assert!(matches!(&member.body, EventBody::UserMessage { earlier, .. } if earlier.is_empty()));

    updates
        .send(vec![
            group(7, "member", true, Action::SelectSession("private".into())),
            group(8, "owner", true, Action::Sessions),
        ])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(8));
    // A member's command expects no answer, so it is ignored rather than refused.
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "A group chat always uses its own Session.");
    receipt.result.send(Ok(())).unwrap();
    let mut replay = group(10, "owner", true, Action::Everyone(Some(true)));
    replay.key = "open".into();
    updates
        .send(vec![
            group(9, "owner", true, Action::Everyone(Some(false))),
            replay,
            group(11, "member", true, Action::Message("closed".into())),
            group(12, "owner", true, Action::Everyone(None)),
        ])
        .await
        .unwrap();
    let receipt = receive(&mut sending).await;
    assert_eq!(
        receipt.text,
        "Only you can ask the Agent in this group now."
    );
    receipt.result.send(Ok(())).unwrap();
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "Only you can ask the Agent in this group.");
    receipt.result.send(Ok(())).unwrap();
    assert_eq!(receive(&mut polling).await, json!(12));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    let mut back = group(13, "owner", true, Action::Backtrack);
    back.input.reply_to = Some("6".into());
    updates.send(vec![back]).await.unwrap();
    receive(&mut polling).await;
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "Back at that message; replying again.");
    receipt.result.send(Ok(())).unwrap();
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();

    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_an_attachment_read_and_groups_without_a_profile_explain_the_setting() {
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut fetching,
        ..
    } = harness(dir.path(), ScriptedProvider::new(vec![])).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![group(1, "owner", true, photo())])
        .await
        .unwrap();
    // The reader fetches for every message that may drive a Session; the connection refuses it.
    receive(&mut fetching)
        .await
        .result
        .send(Ok(AttachmentContent::Unavailable("not needed".into())))
        .unwrap();
    receive(&mut polling).await;
    let receipt = receive(&mut sending).await;
    assert_eq!(
        receipt.text,
        "Set group_profile for this connection to use the Agent in group chats."
    );
    receipt.result.send(Ok(())).unwrap();
    updates.send(vec![update(2, photo())]).await.unwrap();
    let fetch = receive(&mut fetching).await;
    tokio::time::timeout(Duration::from_secs(10), channel.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(fetch.result.is_closed());
    assert!(kernel.sessions().await.unwrap().is_empty());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_rejected_protocol_state_resets_once_and_duplicate_commands_advance_the_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("later answer")]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider.clone()).await;
    let command = |offset, name: &str| {
        let mut value = update(offset, Action::SelectSession(name.into()));
        value.key = format!("select-{offset}");
        value
    };
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![command(1, "work"), command(2, "later")])
        .await
        .unwrap();
    for _ in 0..2 {
        receive(&mut sending).await.result.send(Ok(())).unwrap();
    }
    assert_eq!(receive(&mut polling).await, json!(2));
    channel.shutdown().await.unwrap();
    *adapter.invalid_state.lock().await = Some(Some(json!(2)));
    let channel = start(&kernel, &adapter);
    assert_eq!(receive(&mut polling).await, json!(2));
    assert_eq!(receive(&mut polling).await, Value::Null);
    assert!(kernel.connection("controlled:bot").await.unwrap().unwrap()["protocol"].is_null());
    updates
        .send(vec![command(1, "work"), command(2, "later")])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(2));
    assert!(sending.try_recv().is_err());
    updates
        .send(vec![update(3, Action::Message("new input".into()))])
        .await
        .unwrap();
    assert_eq!(receive(&mut polling).await, json!(3));
    let answer = receive(&mut sending).await;
    assert_eq!(answer.text, "later answer");
    answer.result.send(Ok(())).unwrap();
    let work = kernel.open_session("work").await.unwrap();
    let later = kernel.open_session("later").await.unwrap();
    assert!(kernel.log(work.id, None).await.unwrap().is_empty());
    assert!(kernel.log(later.id, None).await.unwrap().iter().any(|entry| matches!(&entry.body, EntryBody::EventConsumed { event } if matches!(&event.body, EventBody::UserMessage { text, .. } if text == "new input"))));
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
