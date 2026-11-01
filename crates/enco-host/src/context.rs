//! Standing instructions, the owner's and each profile's, read once per Round.
//!
//! A profile's file is found in the same listing that status reports, so both agree whatever the
//! file system's case rules.
use crate::development::{EntryError, EntryProblem};
use async_trait::async_trait;
use enco_core::ProfileName;
use enco_kernel::{ContextError, ContextQuery, ContextSource};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The extension of a profile-instructions file, `<profile>.md`.
const EXTENSION: &str = "md";

/// Supplies either the owner instructions or the current Session's profile instructions.
pub struct InstructionsContextSource {
    source: Instructions,
}

enum Instructions {
    Owner(PathBuf),
    Profile(PathBuf),
}

impl InstructionsContextSource {
    /// Read the owner instructions from this file.
    pub fn owner(path: PathBuf) -> Self {
        Self {
            source: Instructions::Owner(path),
        }
    }

    /// Read the file this `profiles` directory lists for the Session's profile, if any.
    pub fn profile(directory: PathBuf) -> Self {
        Self {
            source: Instructions::Profile(directory),
        }
    }
}

#[async_trait]
impl ContextSource for InstructionsContextSource {
    type Supply = Option<String>;

    async fn contribute(&self, query: &ContextQuery) -> Result<Option<String>, ContextError> {
        let path = match &self.source {
            Instructions::Owner(path) => path.clone(),
            Instructions::Profile(directory) => {
                let files = list(directory)
                    .await
                    .map_err(|error| ContextError(error.to_string()))?;
                match files
                    .into_iter()
                    .find(|file| file.profile.as_ref() == Some(&query.session.profile))
                {
                    Some(file) => file.path,
                    None => return Ok(None),
                }
            }
        };
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ContextError(format!("{}: {e}", path.display()))),
        }
    }
}

/// The profile-instructions files in the home, as status reports them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileInstructionFiles {
    /// The files in `profiles`, sorted; none when the directory is missing.
    Listed(Vec<ProfileInstructionFile>),
    /// Why `profiles` cannot be listed.
    Unreadable(EntryError),
}

/// A `.md` file in `profiles`.
#[derive(Debug, Serialize)]
pub struct ProfileInstructionFile {
    /// Where the file is.
    pub path: PathBuf,
    /// The profile it holds instructions for; absent when its name is not a valid profile name.
    pub profile: Option<ProfileName>,
}

/// List the files in this `profiles` directory, each with the profile it would serve.
pub async fn profile_instruction_files(directory: &Path) -> ProfileInstructionFiles {
    match list(directory).await {
        Ok(files) => ProfileInstructionFiles::Listed(files),
        Err(error) => ProfileInstructionFiles::Unreadable(error),
    }
}

async fn list(directory: &Path) -> Result<Vec<ProfileInstructionFile>, EntryError> {
    let io = |error: std::io::Error| EntryError {
        path: directory.to_owned(),
        problem: EntryProblem::Io(error.to_string()),
    };
    let mut entries = match tokio::fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(error)),
    };
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(io)? {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == EXTENSION)
        {
            let profile = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse().ok());
            files.push(ProfileInstructionFile { path, profile });
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}
