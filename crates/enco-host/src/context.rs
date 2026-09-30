use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use std::path::PathBuf;

/// Supplies the owner's current standing instructions once per Round.
pub struct InstructionsContextSource {
    path: PathBuf,
}

impl InstructionsContextSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl ContextSource for InstructionsContextSource {
    async fn contribute(&self, _query: &ContextQuery) -> Result<Contribution, ContextError> {
        let path = &self.path;
        match tokio::fs::read_to_string(path).await {
            Ok(text) => Ok(Contribution {
                candidates: vec![Candidate {
                    id: "instructions:AGENTS.md".into(),
                    kind: CandidateKind::Instruction,
                    text,
                }],
                omitted: vec![],
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Contribution::default()),
            Err(e) => Err(ContextError(format!("{}: {e}", path.display()))),
        }
    }
}
