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
    assert!(status.stopped.is_none());
    assert_eq!(status.recent_failures.len(), 1);
    assert!(matches!(
        status.recent_failures[0].settlement.outcome,
        Settlement::Failed { .. }
    ));
    kernel.shutdown().await.unwrap();
}
