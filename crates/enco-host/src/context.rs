use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use std::path::PathBuf;

/// Supplies the owner's current standing instructions once per Round.
pub struct WorkspaceContextSource {
    workspace: PathBuf,
}

impl WorkspaceContextSource {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl ContextSource for WorkspaceContextSource {
    async fn contribute(&self, _query: &ContextQuery) -> Result<Contribution, ContextError> {
        let path = self.workspace.join("AGENTS.md");
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => Ok(Contribution {
                candidates: vec![Candidate {
                    id: "workspace:AGENTS.md".into(),
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
