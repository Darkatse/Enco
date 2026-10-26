//! The owner's standing instructions as a context source, read again for each Round.
use async_trait::async_trait;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use std::path::PathBuf;

/// Supplies the owner's current standing instructions once per Round.
pub struct InstructionsContextSource {
    path: PathBuf,
}

impl InstructionsContextSource {
    /// Read the standing instructions from the file at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl ContextSource for InstructionsContextSource {
    type Supply = Option<String>;

    async fn contribute(&self, _query: &ContextQuery) -> Result<Option<String>, ContextError> {
        let path = &self.path;
        match tokio::fs::read_to_string(path).await {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ContextError(format!("{}: {e}", path.display()))),
        }
    }
}
