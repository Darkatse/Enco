use super::*;
use std::collections::BTreeSet;

#[tokio::test]
async fn presences_and_refreshes_share_each_tick_and_every_epilogue_survives_rate_limiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = delegated(dir.path(), 4).await;
    let (channel, parent, first) = begin(&mut s, settings()).await;
    opened(&mut s).await;
    parent.respond(calls(
        ["second", "third", "fourth"]
            .map(|to| {
                call(
                    "session_send",
                    &json!({"to":to,"text":"work","profile":"worker"}).to_string(),
                )
            })
            .into(),
    ));
    let mut prompts = vec![first];
    for _ in 0..3 {
        prompts.push(receive(&mut s.child).await);
    }
    let _parent = receive(&mut s.parent).await;
    let mut targets = BTreeSet::from(["child-topic".to_owned()]);
    for _ in 0..3 {
        let open = receive(&mut s.host.opening).await;
        let target = open.session.last().unwrap().clone();
        targets.insert(target.clone());
        open.result.send(Ok(Some(target))).unwrap();
        let card = receive(&mut s.host.activity).await;
        assert!(!card.replies_here);
        card.result
            .send(Ok(Some(format!("{}-card", card.target))))
            .unwrap();
    }
    for prompt in prompts {
        prompt.respond(calls(vec![call("message_send", r#"{"text":"partial"}"#)]));
    }
    let mut final_prompts = Vec::new();
    for _ in 0..4 {
        final_prompts.push(receive(&mut s.child).await);
    }
    tokio::time::pause();
    // Each tick sends one batch: the parent's presence where its reply goes, then one refresh,
    // in turn.
    let mut refreshed = BTreeSet::new();
    for _ in 0..4 {
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(receive(&mut s.host.presence).await, "chat");
        let card = receive(&mut s.host.activity).await;
        assert!(
            matches!(&card.update, ChannelActivityUpdate::View(view) if view.tools.iter().any(|tool| tool.name == "message_send"))
        );
        assert!(refreshed.insert(card.target.clone()));
        // With the clock paused, the timeout elapses only once every task has gone idle, long
        // before the next tick.
        assert!(
            tokio::time::timeout(Duration::from_millis(10), s.host.activity.recv())
                .await
                .is_err()
        );
        card.result.send(Ok(None)).unwrap();
    }
    assert_eq!(refreshed, targets);
    for prompt in final_prompts {
        prompt.respond(reply("done"));
    }
    let mut epilogues = Vec::new();
    for _ in 0..4 {
        let epilogue = receive(&mut s.host.activity).await;
        assert!(matches!(
            epilogue.update,
            ChannelActivityUpdate::Epilogue(_)
        ));
        epilogues.push(epilogue);
    }
    for epilogue in epilogues {
        epilogue.result.send(Err(limited())).unwrap();
    }
    tokio::time::advance(Duration::from_secs(9)).await;
    let mut finished = BTreeSet::new();
    for _ in 0..4 {
        let epilogue = receive(&mut s.host.activity).await;
        assert!(matches!(
            epilogue.update,
            ChannelActivityUpdate::Epilogue(_)
        ));
        assert!(finished.insert(epilogue.target));
        epilogue.result.send(Ok(None)).unwrap();
    }
    assert_eq!(finished, targets);
    tokio::time::resume();
    channel.shutdown().await.unwrap();
    s.host.kernel.shutdown().await.unwrap();
}
