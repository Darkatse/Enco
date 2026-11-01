use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, fmt, ops::Deref, str::FromStr};

/// Whether a name is one nonempty segment, without slashes, whitespace or controls, and not `.` or `..`.
pub fn is_name_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c == '/' || c.is_whitespace() || c.is_control())
}

/// A validated profile name, also used to select its profile instructions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileName(String);

/// A profile name outside the single-segment naming rule.
#[derive(Debug, thiserror::Error)]
#[error(
    "profile name must be one nonempty segment without '/', whitespace or control characters, and not '.' or '..': {0}"
)]
pub struct InvalidProfileName(String);

impl FromStr for ProfileName {
    type Err = InvalidProfileName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if !is_name_segment(name) {
            return Err(InvalidProfileName(name.into()));
        }
        Ok(Self(name.into()))
    }
}

impl ProfileName {
    /// The validated name used in configuration and instruction paths.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ProfileName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Borrow<str> for ProfileName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for ProfileName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<ProfileName> for String {
    fn from(name: ProfileName) -> Self {
        name.0
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for ProfileName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
