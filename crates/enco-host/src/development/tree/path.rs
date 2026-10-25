//! Paths inside a file tree, held to one canonical form so equal trees hash equally.
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    fmt,
    path::{Component, Path},
    str::FromStr,
};

/// A canonical UTF-8 path relative to a file tree, with `/` as its separator.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SourcePath(String);

/// Text that is not a source path: an empty, `.` or `..` part, a `\\`, or an absolute path.
#[derive(Debug, thiserror::Error)]
#[error("source path must be a normalized relative path: {0}")]
pub struct InvalidSourcePath(String);

impl FromStr for SourcePath {
    type Err = InvalidSourcePath;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || text.contains('\\')
            || Path::new(text)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(InvalidSourcePath(text.into()));
        }
        Ok(Self(text.into()))
    }
}

impl SourcePath {
    /// The path as text, with `/` between its parts.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for SourcePath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for SourcePath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for SourcePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for SourcePath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
