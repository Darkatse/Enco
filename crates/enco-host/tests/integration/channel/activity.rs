use super::*;

#[tokio::test]
async fn menu_and_typing_failures_do_not_block_deliveries_and_typing_stops_with_the_run() {
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
        ..
    } = harness(dir.path(), provider).await;
    adapter.menu_failure.store(true, Ordering::SeqCst);
    adapter.typing_failure.store(true, Ordering::SeqCst);
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
    let scopes = adapter.menu_scopes.lock().await;
    assert_eq!(scopes.iter().filter(|(direct, _)| *direct).count(), 3);
    assert_eq!(scopes.iter().filter(|(_, group)| *group).count(), 3);
    drop(scopes);
    updates
        .send(vec![update(1, Action::Message("work".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    adapter.typing_notify.notified().await;
    let progress = receive(&mut sending).await;
    assert_eq!(progress.text, "working");
    progress.result.send(Ok(())).unwrap();
    tokio::time::pause();
    let before_tick = adapter.typed.load(Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(4)).await;
    adapter.typing_notify.notified().await;
    assert!(adapter.typed.load(Ordering::SeqCst) > before_tick);
    let session = kernel.open_session("main").await.unwrap();
    let mut entries = kernel.subscribe(session.id).unwrap();
    tokio::fs::write(&gate, b"done\n").await.unwrap();
    let final_reply = receive(&mut sending).await;
    assert_eq!(final_reply.text, "finished");
    final_reply.result.send(Ok(())).unwrap();
    finish(&mut entries).await;
    updates
        .send(vec![update(2, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    let after_run = adapter.typed.load(Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(12)).await;
    updates
        .send(vec![update(3, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    assert_eq!(adapter.typed.load(Ordering::SeqCst), after_run);
    assert!(channel.status().await.unwrap().recent_failures.is_empty());
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
