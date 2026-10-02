mod support;

use enco_core::*;
use enco_kernel::Store;
use support::*;

#[tokio::test]
async fn recorded_requests_match_provider_input_and_tool_history_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let write = call("fs_write", r#"{"path":"note.txt","content":"hello"}"#);
    let provider = ScriptedProvider::new(vec![calls(vec![write.clone()]), reply("done")]);
    let (kernel, _) = kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "write a note".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let requests = provider.requests.lock().unwrap().clone();
    let starts: Vec<_> = entries
        .iter()
        .filter_map(|e| match e.body {
            EntryBody::AttemptStarted { attempt, .. } => Some(attempt),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), requests.len());
    for (attempt, request) in starts.iter().zip(&requests) {
        assert_eq!(
            &kernel
                .inspect(session.id, Some(*attempt))
                .await
                .unwrap()
                .request,
            request
        );
    }
    kernel.shutdown().await.unwrap();
    drop(kernel);

    let provider = ScriptedProvider::new(vec![reply("continued")]);
    let (kernel, _) = support::kernel(dir.path(), provider.clone()).await;
    assert_eq!(kernel.log(session.id, None).await.unwrap(), entries);
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "continue".into())
        .await
        .unwrap();
    finish(&mut rx).await;
    let restored = provider.requests.lock().unwrap()[0].messages.clone();
    let tool_result = requests[1]
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .unwrap();
    assert!(restored.contains(tool_result));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn file_pages_preserve_complete_lines_and_oversized_lines_use_the_shared_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, store) = kernel(dir.path(), provider.clone()).await;
    let expected: Vec<_> = (0..80)
        .map(|i| format!("{i}: {}", "中文".repeat(100)))
        .collect();
    std::fs::write(dir.path().join("workspace/pages.txt"), expected.join("\n")).unwrap();
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    let mut offset = 1;
    let mut read_lines = vec![];
    loop {
        provider.steps.lock().unwrap().extend([
            calls(vec![call(
                "fs_read",
                &serde_json::json!({"path": "pages.txt", "offset": offset}).to_string(),
            )]),
            reply("page read"),
        ]);
        kernel
            .submit(session.id, EventId::new(), "next page".into())
            .await
            .unwrap();
        let entries = finish(&mut rx).await;
        let (content, full) = entries
            .iter()
            .find_map(|entry| match &entry.body {
                EntryBody::ToolCallSettled {
                    outcome: Settlement::Ok,
                    content,
                    full,
                    ..
                } => Some((content, full)),
                _ => None,
            })
            .unwrap();
        assert!(full.is_none());
        assert!(content.len() <= 16 * 1024);
        let (header, body) = content.split_once('\n').unwrap();
        read_lines.extend(body.lines().map(str::to_owned));
        let Some((_, next)) = header.split_once("continue at offset ") else {
            break;
        };
        let next: usize = next.trim_end_matches(']').parse().unwrap();
        assert!(next > offset);
        assert_eq!(next, read_lines.len() + 1);
        offset = next;
    }
    assert!(offset > 1, "the file must require multiple pages");
    assert_eq!(read_lines, expected);

    let long_line = "界".repeat(10_000);
    std::fs::write(
        dir.path().join("workspace/long.txt"),
        format!("{long_line}\ntail"),
    )
    .unwrap();
    provider.steps.lock().unwrap().extend([
        calls(vec![call("fs_read", r#"{"path":"long.txt"}"#)]),
        reply("long line read"),
    ]);
    kernel
        .submit(session.id, EventId::new(), "read the long line".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let (content, hash) = entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Ok,
                content,
                full: Some(hash),
                ..
            } => Some((content, hash)),
            _ => None,
        })
        .unwrap();
    let header = "[lines 1-1 of 2; continue at offset 2]\n";
    assert!(content.starts_with(header));
    assert!(content.len() <= 16 * 1024);
    assert_eq!(
        store.get_blob(hash).await.unwrap(),
        format!("{header}{long_line}").as_bytes()
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn unavailable_calls_and_invalid_arguments_are_settled_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![
            call("missing", "{}"),
            call("fs_write", "[]"),
            call(
                "shell_exec",
                r#"{"command":"touch wrong-directory","workdir":"/tmp"}"#,
            ),
        ]),
        reply("understood"),
    ]);
    let (kernel, _) = kernel(dir.path(), provider).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "test failures".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e.body, EntryBody::ToolCallStarted { .. }))
    );
    assert!(!dir.path().join("workspace/wrong-directory").exists());
    let codes: Vec<_> = entries
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Failed { failure },
                ..
            } => Some(failure.code.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            code::TOOL_UNAVAILABLE,
            code::TOOL_INVALID_ARGUMENTS,
            code::TOOL_INVALID_ARGUMENTS
        ]
    );
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn retries_reuse_the_frozen_plan_and_nonretryable_failures_end_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        Err(Failure {
            code: code::PROVIDER_NETWORK.into(),
            message: "offline".into(),
            retryable: true,
        }),
        reply("ok"),
        Err(Failure {
            code: code::PROVIDER_AUTH.into(),
            message: "bad credentials".into(),
            retryable: false,
        }),
    ]);
    let (kernel, _) = kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "hello".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    let plans: Vec<_> = entries
        .iter()
        .filter_map(|e| match e.body {
            EntryBody::AttemptStarted { plan, .. } => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0], plans[1]);
    {
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests[0], requests[1]);
    }
    kernel
        .submit(session.id, EventId::new(), "again".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert!(matches!(
        &entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Failed { failure },
            ..
        } if failure.code == code::PROVIDER_AUTH
    ));
    kernel.shutdown().await.unwrap();
}

struct AlteredDefinition(enco_host::FactoryComposer);

impl enco_kernel::Composer for AlteredDefinition {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "altered-definition".into(),
            version: "test".into(),
        }
    }

    fn compose(
        &self,
        input: &enco_kernel::ComposeInput,
    ) -> Result<enco_kernel::Composition, enco_kernel::ComposeError> {
        let mut result = self.0.compose(input)?;
        if let enco_kernel::Composition::Plan(plan) = &mut result {
            plan.tools[0].1.description = "Not the actual tool contract".into();
        }
        Ok(result)
    }
}

#[tokio::test]
async fn changed_tool_definitions_are_rejected_before_any_provider_request() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps| {
        deps.composer = std::sync::Arc::new(AlteredDefinition(enco_host::FactoryComposer::new(
            dir.path().join("workspace"),
            dir.path().join("AGENTS.md"),
        )))
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "hello".into())
        .await
        .unwrap();
    let log = finish(&mut rx).await;
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(matches!(
        &log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Failed { failure },
            ..
        } if failure.code == code::PLAN_INVALID
    ));
    kernel.shutdown().await.unwrap();
}
