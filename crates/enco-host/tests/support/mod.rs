#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "shared test support: each test binary uses a subset of these helpers, which fail the test by panicking"
)]
use async_trait::async_trait;
use enco_core::*;
use enco_host::*;
use enco_kernel::*;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;

pub struct ScriptedProvider {
    pub embedding_failure: std::sync::atomic::AtomicBool,
    pub embedding_dimensions: std::sync::atomic::AtomicUsize,
    pub steps: Mutex<VecDeque<Result<Completion, Failure>>>,
    pub requests: Mutex<Vec<ProviderRequest>>,
}

impl ScriptedProvider {
    pub fn new(steps: Vec<Result<Completion, Failure>>) -> Arc<Self> {
        Arc::new(Self {
            embedding_failure: std::sync::atomic::AtomicBool::new(false),
            embedding_dimensions: std::sync::atomic::AtomicUsize::new(64),
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(vec![]),
        })
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn code(&self) -> CodeRef {
        CodeRef::Native {
            name: "scripted".into(),
            version: "test".into(),
        }
    }

    async fn complete(&self, request: ProviderRequest) -> Result<Completion, Failure> {
        self.requests.lock().unwrap().push(request);
        self.steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra provider request")
    }

    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure> {
        if self
            .embedding_failure
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Failure {
                code: code::PROVIDER_NETWORK.into(),
                message: "embedding service unavailable".into(),
                retryable: true,
            });
        }
        Ok(vectors(
            &inputs,
            self.embedding_dimensions
                .load(std::sync::atomic::Ordering::SeqCst),
        ))
    }
}

pub fn reply(text: &str) -> Result<Completion, Failure> {
    Ok(Completion {
        message: Message::text(Role::Assistant, text),
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
            cached_input_tokens: None,
        },
        stop: StopReason::EndTurn,
    })
}

pub fn call(name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: CallId::new(),
        provider_id: format!("provider-{}", CallId::new()),
        name: name.into(),
        arguments: args.into(),
    }
}

pub fn calls(calls: Vec<ToolCall>) -> Result<Completion, Failure> {
    Ok(Completion {
        message: Message {
            role: Role::Assistant,
            parts: calls.into_iter().map(Part::ToolCall).collect(),
        },
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
            cached_input_tokens: None,
        },
        stop: StopReason::ToolCalls,
    })
}

pub async fn kernel(root: &Path, provider: Arc<dyn Provider>) -> (Kernel, Arc<SqliteStore>) {
    kernel_with(root, provider, |_, _| {}).await
}

pub async fn kernel_with(
    root: &Path,
    provider: Arc<dyn Provider>,
    configure: impl FnOnce(&mut KernelDeps, &mut Budget),
) -> (Kernel, Arc<SqliteStore>) {
    let store = Arc::new(
        SqliteStore::open(root.join("enco.db"), root.join("blobs"))
            .await
            .unwrap(),
    );
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut deps = KernelDeps {
        store: store.clone(),
        provider,
        composer: Arc::new(FactoryComposer::new(
            workspace.clone(),
            root.join("AGENTS.md"),
        )),
        context: vec![Arc::new(InstructionsContextSource::new(
            root.join("AGENTS.md"),
        ))],
        tools: native_tools(workspace),
        lifeline: LIFELINE.iter().map(|s| s.to_string()).collect(),
        clock: Arc::new(SystemClock),
    };
    let mut budget = Budget {
        context_tokens: 128000,
        max_output_tokens: 8192,
    };
    configure(&mut deps, &mut budget);
    let config = KernelConfig::new(budget, 24).unwrap();
    (Kernel::start(deps, config).await.unwrap(), store)
}

pub async fn finish(rx: &mut broadcast::Receiver<Entry>) -> Vec<Entry> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut entries = vec![];
        loop {
            let entry = rx.recv().await.unwrap();
            let ended = matches!(entry.body, EntryBody::RunEnded { .. });
            entries.push(entry);
            if ended {
                return entries;
            }
        }
    })
    .await
    .expect("Run did not end")
}

pub fn vectors(inputs: &[String], dimensions: usize) -> Vec<Vec<f32>> {
    inputs
        .iter()
        .map(|text| {
            let mut vector = vec![0.0f32; dimensions];
            let chars: Vec<_> = text.chars().collect();
            for pair in chars.windows(2) {
                vector[((pair[0] as usize).wrapping_mul(31) + pair[1] as usize) % dimensions] +=
                    1.0;
            }
            let norm = vector.iter().map(|n| n * n).sum::<f32>().sqrt();
            if norm > 0.0 {
                for value in &mut vector {
                    *value /= norm;
                }
            } else {
                vector[0] = 1.0;
            }
            vector
        })
        .collect()
}

pub fn is_reminder(entry: &Entry, id: ScheduleId) -> bool {
    let EntryBody::EventConsumed { event } = &entry.body else {
        return false;
    };
    matches!(event.body, EventBody::Reminder { schedule, .. } if schedule == id)
}
