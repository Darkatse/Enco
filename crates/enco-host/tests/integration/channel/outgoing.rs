use super::*;

#[tokio::test]
async fn settled_message_arrives_before_the_same_rounds_shell_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("message_send", r#"{"text":"working","file":"picture.png"}"#),
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
    let workspace = dir.path().join("workspace");
    std::fs::write(workspace.join("picture.png"), PNG).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(workspace.join("gate"))
            .status()
            .unwrap()
            .success()
    );
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("start".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let progress = receive(&mut sending).await;
    assert_eq!(progress.text, "working");
    assert_eq!(
        progress.file,
        Some(FileData {
            name: "picture.png".into(),
            data: PNG.to_vec()
        })
    );
    let session = kernel.open_session("main").await.unwrap();
    let entries = kernel.log(session.id, None).await.unwrap();
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e.body, EntryBody::RoundEnded { .. }))
    );
    progress.result.send(Ok(())).unwrap();
    let mut entries = kernel.subscribe(session.id).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::fs::write(workspace.join("gate"), b"done\n"),
    )
    .await
    .unwrap()
    .unwrap();
    let final_reply = receive(&mut sending).await;
    assert_eq!(final_reply.text, "finished");
    assert!(final_reply.file.is_none());
    final_reply.result.send(Ok(())).unwrap();
    finish(&mut entries).await;
    channel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_delivers_recorded_files_and_continues_after_missing_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        reply("seed"),
        calls(vec![
            call("message_send", r#"{"file":"missing.txt"}"#),
            call("message_send", r#"{"file":"kept.txt"}"#),
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
    let workspace = dir.path().join("workspace");
    std::fs::write(workspace.join("missing.txt"), "missing bytes").unwrap();
    std::fs::write(workspace.join("kept.txt"), "original bytes").unwrap();
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("seed".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut entries = kernel.subscribe(session.id).unwrap();
    kernel
        .accept(
            &[Event {
                id: EventId::new(),
                session: session.id,
                source: EventSource::Scheduler,
                body: EventBody::Reminder {
                    schedule: ScheduleId::new(),
                    due_at: enco_core::Utc::now(),
                    skipped: 0,
                    text: "continue".into(),
                },
                received_at: enco_core::Utc::now(),
            }],
            None,
        )
        .await
        .unwrap();
    let entries = finish(&mut entries).await;
    let missing = entries
        .iter()
        .find_map(|e| match &e.body {
            EntryBody::ToolCallSettled {
                outgoing:
                    Some(Outgoing {
                        file: Some(file), ..
                    }),
                ..
            } => Some(file),
            _ => None,
        })
        .unwrap();
    let path = enco_host::blob_path(&dir.path().join("blobs"), &missing.content);
    std::fs::remove_file(path).unwrap();
    std::fs::write(workspace.join("kept.txt"), "changed bytes").unwrap();
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    let file = receive(&mut sending).await;
    assert_eq!(file.text, "");
    assert_eq!(
        file.file,
        Some(FileData {
            name: "kept.txt".into(),
            data: b"original bytes".to_vec()
        })
    );
    file.result.send(Ok(())).unwrap();
    let reply = receive(&mut sending).await;
    assert_eq!(reply.text, "finished");
    reply.result.send(Ok(())).unwrap();
    channel.shutdown().await.unwrap();
    let status = channel.status().await.unwrap();
    assert!(matches!(
        status.state,
        enco_host::channel::ChannelState::Stopped { failure: None }
    ));
    assert_eq!(status.recent_failures.len(), 1);
    assert!(matches!(
        status.recent_failures[0].settlement.outcome,
        Settlement::Failed { .. }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_split_failure_settles_as_failed_before_any_send_instead_of_recovering_as_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        ..
    } = harness(dir.path(), ScriptedProvider::new(vec![reply("reply")])).await;
    adapter.split_failure.store(true, Ordering::SeqCst);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("input".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    adapter.split_called.notified().await;
    channel.shutdown().await.unwrap();
    let failures = channel.status().await.unwrap().recent_failures;
    assert_eq!(adapter.sent.load(Ordering::SeqCst), 0);
    assert_eq!(failures.len(), 1);
    assert!(
        matches!(&failures[0].settlement.outcome, Settlement::Failed { failure } if failure.code == code::PLUGIN_TRAP)
    );
    assert!(kernel.connection("controlled:bot").await.unwrap().unwrap()["sending"].is_null());
    kernel.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn deliveries_and_receipts_share_pacing_and_started_waits_finish_on_stop() {
    use std::{future::Future, task::Poll};
    let dir = tempfile::tempdir().unwrap();
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = harness(
        dir.path(),
        ScriptedProvider::new(vec![reply("first\nsecond")]),
    )
    .await;
    *adapter.pacing.lock().unwrap() = Some(enco_kernel::ChannelPacingGap {
        minimum_ms: 1500,
        jitter_ms: 0,
    });
    adapter.split_lines.store(true, Ordering::SeqCst);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("start".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let first = receive(&mut sending).await;
    assert_eq!(first.text, "first");
    // Consume the delivery's split, so the next notification is the receipt's.
    adapter.split_called.notified().await;
    // The owner accepts a command while delivery holds the send gate.
    updates
        .send(vec![update(2, Action::Sessions)])
        .await
        .unwrap();
    receive(&mut polling).await;
    adapter.split_called.notified().await;
    assert!(sending.try_recv().is_err());

    // Polled once, shutdown signals stop and then waits for the started sends.
    let mut shutdown = std::pin::pin!(channel.shutdown());
    std::future::poll_fn(|cx| {
        assert!(shutdown.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    // The first part takes longer than the gap, so a gap counted from its start would be over.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let finished = tokio::time::Instant::now();
    first.result.send(Ok(())).unwrap();
    // This receipt already started; its gate wait and send survive cancellation.
    let receipt = receive(&mut sending).await;
    assert_eq!(finished.elapsed(), Duration::from_millis(1500));
    assert_ne!(receipt.text, "second");
    receipt.result.send(Ok(())).unwrap();
    shutdown.await.unwrap();
    assert!(sending.try_recv().is_err());
    let failures = channel.status().await.unwrap().recent_failures;
    assert_eq!(failures.len(), 1);
    assert!(
        matches!(&failures[0].settlement.outcome, Settlement::Failed { failure } if failure.code == code::CANCELLED)
    );
    assert_eq!(failures[0].settlement.messages, vec![first.id]);
    kernel.shutdown().await.unwrap();
}
