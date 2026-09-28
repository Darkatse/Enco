mod support;

use enco_core::*;
use enco_kernel::Store;
use support::*;

#[tokio::test]
async fn replies_and_tool_calls_follow_one_durable_path() {
    let dir = tempfile::tempdir().unwrap();
    let write = call("fs_write", r#"{"path":"note.txt","content":"hello"}"#);
    let provider = ScriptedProvider::new(vec![calls(vec![write.clone()]), reply("done")]);
    let (kernel, store) = kernel(dir.path(), provider.clone()).await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), "write a note".into())
        .await
        .unwrap();
    let entries = finish(&mut rx).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("workspace/note.txt")).unwrap(),
        "hello"
    );
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages.iter().any(|message| {
        message.parts.iter().any(|part| {
            matches!(
                part,
                Part::ToolResult(result)
                    if result.call == write.id && result.provider_id == write.provider_id
            )
        })
    }));
    let starts: Vec<_> = entries
        .iter()
        .filter_map(|e| match e.body {
            EntryBody::AttemptStarted { plan, .. } => Some(plan),
            _ => None,
        })
        .collect();
    let plan: ContextPlan =
        serde_json::from_slice(&store.get_blob(&starts[0]).await.unwrap()).unwrap();
    assert_eq!(
        plan.tools
            .iter()
            .map(|(_, s)| s.clone())
            .collect::<Vec<_>>(),
        requests[0].tools
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
                outcome: Outcome::Failed { failure },
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
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps, _| {
        deps.composer = std::sync::Arc::new(AlteredDefinition(enco_host::FactoryComposer::new(
            dir.path().join("workspace"),
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
