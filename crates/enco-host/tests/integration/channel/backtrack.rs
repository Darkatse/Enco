use super::*;
use enco_kernel::ProviderRequest;

fn texts(request: &ProviderRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .map(Message::joined_text)
        .filter(|text| !text.is_empty())
        .collect()
}

/// Send one owner message and confirm the Agent's reply, returning the reply's protocol ID.
async fn exchange(
    updates: &mpsc::Sender<Vec<Update>>,
    polling: &mut mpsc::UnboundedReceiver<Value>,
    sending: &mut mpsc::UnboundedReceiver<Send>,
    offset: u32,
    text: &str,
) -> String {
    updates
        .send(vec![update(offset, Action::Message(text.into()))])
        .await
        .unwrap();
    receive(polling).await;
    let reply = receive(sending).await;
    let id = reply.id.clone();
    reply.result.send(Ok(())).unwrap();
    id
}

#[tokio::test]
async fn a_reply_with_backtrack_goes_back_to_that_agent_or_owner_message() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(["first", "second", "third", "again"].map(reply).into());
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(dir.path(), provider.clone()).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    let first = exchange(&updates, &mut polling, &mut sending, 1, "one").await;
    exchange(&updates, &mut polling, &mut sending, 2, "two").await;

    // An Agent message is found through the delivery that sent it.
    updates.send(vec![backtrack(3, Some(first))]).await.unwrap();
    receive(&mut polling).await;
    let receipt = receive(&mut sending).await;
    assert!(receipt.text.starts_with("Back at that message."));
    receipt.result.send(Ok(())).unwrap();
    exchange(&updates, &mut polling, &mut sending, 4, "three").await;
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one", "first", "three"]
    );

    // The owner's message is found through the input it became, even in the part left behind.
    updates
        .send(vec![backtrack(5, Some("2".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let receipt = receive(&mut sending).await;
    assert_eq!(receipt.text, "Back at your message; replying again.");
    receipt.result.send(Ok(())).unwrap();
    let again = receive(&mut sending).await;
    assert_eq!(again.text, "again");
    again.result.send(Ok(())).unwrap();
    assert_eq!(
        texts(provider.requests.lock().unwrap().last().unwrap()),
        ["one", "first", "two"]
    );
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_backtrack_without_a_recorded_message_in_the_current_session_is_refused() {
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
    } = harness(dir.path(), provider.clone()).await;
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    let first = exchange(&updates, &mut polling, &mut sending, 1, "one").await;
    updates
        .send(vec![update(2, Action::SelectSession("work".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let switched = receive(&mut sending).await;
    let command_receipt = switched.id.clone();
    switched.result.send(Ok(())).unwrap();

    updates
        .send(vec![
            backtrack(3, None),
            backtrack(4, Some(first)),
            backtrack(5, Some(command_receipt)),
        ])
        .await
        .unwrap();
    let mut refusals = Vec::new();
    for _ in 0..3 {
        let receipt = receive(&mut sending).await;
        refusals.push(receipt.text.clone());
        receipt.result.send(Ok(())).unwrap();
    }
    assert_eq!(
        refusals,
        [
            "Send this as a reply to the message to go back to.",
            "That message is in Session main; switch to it first.",
            "That message is not part of any conversation.",
        ]
    );
    // The refusals advance the protocol state without submitting anything.
    assert_eq!(receive(&mut polling).await, json!(5));
    for session in kernel.sessions().await.unwrap() {
        let consumed = kernel.log(session.id, None).await.unwrap();
        let consumed = consumed.iter().filter_map(|entry| match &entry.body {
            EntryBody::EventConsumed { event } => Some(event),
            _ => None,
        });
        let pending = store.pending(session.id).await.unwrap();
        assert!(
            !consumed
                .chain(&pending)
                .any(|event| matches!(event.body, EventBody::Backtrack { .. }))
        );
    }
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

fn backtrack(offset: u32, reply: Option<String>) -> Update {
    let mut value = update(offset, Action::Backtrack);
    value.input.reply_to = reply;
    value
}
