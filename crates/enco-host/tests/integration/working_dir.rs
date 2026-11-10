#![expect(clippy::unwrap_used, reason = "test helpers fail with context")]
use crate::support::*;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::*;
use std::sync::{Arc, atomic::Ordering};

#[tokio::test]
async fn working_directory_changes_are_pinned_per_round_and_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let next = root.path().join("project");
    std::fs::create_dir(&next).unwrap();
    std::fs::write(next.join("marker"), "new-directory").unwrap();
    let provider = Arc::new(PausedProvider::new(
        ScriptedProvider::new(vec![
            calls(vec![call("fs_read", r#"{"path":"marker"}"#)]),
            calls(vec![
                call("fs_read", r#"{"path":"marker"}"#),
                call("shell_exec", r#"{"command":"pwd -P"}"#),
                call("message_send", r#"{"file":"marker"}"#),
            ]),
            reply("done"),
        ]),
        None,
        false,
    ));
    let (kernel, store) = kernel_with(root.path(), provider.clone(), Tz::UTC, |_| {}).await;
    let session = kernel.open_session("main").await.unwrap();
    assert_eq!(session.working_dir.node, store.node().await.unwrap().id);
    assert_eq!(session.working_dir.path(), root.path().join("workspace"));
    std::fs::write(session.working_dir.path().join("marker"), "old-directory").unwrap();
    let mut rx = kernel.subscribe(session.id).await.unwrap();
    provider.pause.store(true, Ordering::SeqCst);
    kernel
        .submit(
            session.id,
            EventId::new(),
            "change directory".into(),
            vec![],
        )
        .await
        .unwrap();
    provider.entered.notified().await;
    let changed = kernel
        .set_working_dir(session.id, next.clone())
        .await
        .unwrap();
    provider.resume.notify_one();
    let entries = finish(&mut rx).await;
    // The command waited for the boundary; the Run went on and completed.
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    let directories: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::RoundStarted { working_dir, .. } => Some(working_dir),
            _ => None,
        })
        .collect();
    assert_eq!(directories, vec![&session.working_dir, &changed, &changed]);
    assert_tool_locations(&entries, &store, &next).await;
    let missing = root.path().join("future-project");
    kernel
        .set_working_dir(session.id, missing.clone())
        .await
        .unwrap();
    assert!(!missing.exists());
    assert!(matches!(
        kernel.set_working_dir(SessionId::new(), next).await,
        Err(KernelError::UnknownSession(_))
    ));
    kernel.shutdown().await.unwrap();
    drop(kernel);
    drop(store);
    let reopened = open_store(root.path()).await;
    assert_eq!(
        reopened
            .session(session.id)
            .await
            .unwrap()
            .unwrap()
            .working_dir
            .path(),
        missing
    );
}

async fn assert_tool_locations(entries: &[Entry], store: &SqliteStore, next: &std::path::Path) {
    let results: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                content,
                outcome: Settlement::Ok,
                ..
            } => Some(content),
            _ => None,
        })
        .collect();
    assert!(results[0].contains("old-directory"));
    assert!(results[1].contains("new-directory"));
    let shell: serde_json::Value = serde_json::from_str(results[2]).unwrap();
    assert_eq!(
        shell["stdout"].as_str().unwrap().trim(),
        std::fs::canonicalize(next).unwrap().to_str().unwrap()
    );
    let sent = entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outgoing: Some(outgoing),
                ..
            } => outgoing.file.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        store.get_blob(&sent.content).await.unwrap(),
        b"new-directory"
    );
}

#[tokio::test]
async fn an_idle_session_takes_a_command_at_once_without_starting_a_run() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, store) = kernel_with(root.path(), provider.clone(), Tz::UTC, |_| {}).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).await.unwrap();
    let changed = kernel
        .set_working_dir(session.id, root.path().join("project"))
        .await
        .unwrap();
    // The command is visible at once in the effective view, before the actor consumes it.
    assert_eq!(
        store
            .session(session.id)
            .await
            .unwrap()
            .unwrap()
            .working_dir,
        changed
    );
    let consumed = tokio::time::timeout(std::time::Duration::from_secs(15), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        &consumed.body,
        EntryBody::EventConsumed { event } if matches!(event.body, EventBody::SetWorkingDir { .. })
    ));
    // After shutdown the actor has finished whatever it would have started.
    kernel.shutdown().await.unwrap();
    let log = store.log(session.id, None).await.unwrap();
    assert_eq!(log.last().unwrap().pos, consumed.pos);
    assert!(
        !log.iter()
            .any(|entry| matches!(entry.body, EntryBody::RunStarted { .. }))
    );
    assert_eq!(
        store
            .committed_session(session.id)
            .await
            .unwrap()
            .unwrap()
            .working_dir,
        changed
    );
    assert!(provider.requests.lock().unwrap().is_empty());
}
