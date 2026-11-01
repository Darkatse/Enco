use crate::NodeId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A node's absolute UTF-8 directory path; availability is checked only when a tool needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawWorkingDir")]
pub struct WorkingDir {
    /// Node whose file system holds the directory.
    pub node: NodeId,
    path: String,
}

#[derive(Deserialize)]
struct RawWorkingDir {
    node: NodeId,
    path: PathBuf,
}

/// A working directory that is not an absolute UTF-8 path.
#[derive(Debug, thiserror::Error)]
#[error("working directory must be an absolute UTF-8 path: {}", .0.display())]
pub struct InvalidWorkingDir(PathBuf);

impl WorkingDir {
    /// Fix a node and absolute UTF-8 path without checking the file system.
    pub fn new(node: NodeId, path: PathBuf) -> Result<Self, InvalidWorkingDir> {
        if !path.is_absolute() {
            return Err(InvalidWorkingDir(path));
        }
        let path = path
            .into_os_string()
            .into_string()
            .map_err(|path| InvalidWorkingDir(path.into()))?;
        Ok(Self { node, path })
    }

    /// Absolute path on the named node, without any availability guarantee.
    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    /// Absolute path as UTF-8, guaranteed by construction.
    pub fn path_str(&self) -> &str {
        &self.path
    }
}

impl TryFrom<RawWorkingDir> for WorkingDir {
    type Error = InvalidWorkingDir;

    fn try_from(raw: RawWorkingDir) -> Result<Self, Self::Error> {
        Self::new(raw.node, raw.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_deserialization_require_absolute_utf8_paths() {
        let node = NodeId::new();
        assert!(WorkingDir::new(node, "relative".into()).is_err());
        assert!(
            serde_json::from_value::<WorkingDir>(serde_json::json!({
                "node": node, "path": "relative"
            }))
            .is_err()
        );
        let directory = WorkingDir::new(node, "/not-required-to-exist".into()).unwrap();
        assert_eq!(
            serde_json::from_value::<WorkingDir>(serde_json::to_value(&directory).unwrap())
                .unwrap(),
            directory
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_cannot_enter_the_working_directory_type() {
        use std::os::unix::ffi::OsStringExt;
        let path = std::ffi::OsString::from_vec(b"/invalid-\xff".to_vec());
        assert!(WorkingDir::new(NodeId::new(), path.into()).is_err());
    }
}
