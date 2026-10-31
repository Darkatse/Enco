use super::*;
use crate::silence::{reminder, skip};

#[tokio::test]
async fn a_skipped_reply_keeps_its_note_but_only_deletes_the_activity_card() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        reply("seed"),
        calls(vec![call(
            "shell_exec",
            r#"{"command":"read line < gate"}"#,
        )]),
        skip("Routine check passed; nothing needs attention."),
        reply("A reminder worth sending"),
    ]);
    let Harness {
        kernel,
        adapter,
        updates,
        mut polling,
        mut sending,
        mut activity,
        ..
    } = harness(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap().id;
    let mut rx = kernel.subscribe(session).unwrap();
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("hello".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    receive(&mut sending).await.result.send(Ok(())).unwrap();
    finish(&mut rx).await;
    channel.shutdown().await.unwrap();

    let gate = dir.path().join("workspace/gate");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&gate)
            .status()
            .unwrap()
            .success()
    );
    adapter.activity_enabled.store(true, Ordering::SeqCst);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    reminder(&kernel, session).await;
    let created = receive(&mut activity).await;
    assert!(created.shown.is_none());
    assert_eq!(created.target, "chat");
    created.result.send(Ok(Some("card".into()))).unwrap();
    tokio::fs::write(&gate, b"done\n").await.unwrap();
    let entries = finish(&mut rx).await;
    let deleted = receive(&mut activity).await;
    assert_eq!(deleted.shown.as_deref(), Some("card"));
    assert!(deleted.view.is_none());
    deleted.result.send(Ok(None)).unwrap();
    channel.shutdown().await.unwrap();
    assert!(sending.try_recv().is_err());
    assert!(entries.iter().any(|entry| matches!(
        entry.body,
        EntryBody::RoundEnded {
            end: RoundEnd::ReplySkipped,
            ..
        }
    )));
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    assert!(entries.iter().any(|entry| matches!(&entry.body,
        EntryBody::AttemptSettled { result: AttemptResult::Completed { message, .. }, .. }
        if message.joined_text() == "Routine check passed; nothing needs attention."
    )));
    {
        let requests = provider.requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| request.tools == requests[0].tools)
        );
    }

    // A later ordinary reply still follows the same route and does not replay the skipped note.
    adapter.activity_enabled.store(false, Ordering::SeqCst);
    let channel = start(&kernel, &adapter);
    receive(&mut polling).await;
    reminder(&kernel, session).await;
    let delivered = receive(&mut sending).await;
    assert_eq!(delivered.text, "A reminder worth sending");
    delivered.result.send(Ok(())).unwrap();
    finish(&mut rx).await;
    channel.shutdown().await.unwrap();
    assert!(sending.try_recv().is_err());
    kernel.shutdown().await.unwrap();
}
