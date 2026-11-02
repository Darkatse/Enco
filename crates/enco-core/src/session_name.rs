use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, fmt, ops::Deref, str::FromStr};

/// A Session's full name, made of validated segments separated by `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct SessionName(String);

impl Ord for SessionName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.split('/').cmp(other.0.split('/'))
    }
}

impl PartialOrd for SessionName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A Session name with an empty or invalid segment.
#[derive(Debug, thiserror::Error)]
#[error(
    "session name must contain nonempty segments without whitespace or controls, and not '.' or '..': {0}"
)]
pub struct InvalidSessionName(String);

impl SessionName {
    /// Validate a single-segment name for a root Session.
    pub fn root(name: &str) -> Result<Self, InvalidSessionName> {
        if !crate::is_name_segment(name) {
            return Err(InvalidSessionName(name.into()));
        }
        Ok(Self(name.into()))
    }

    /// Append one validated child segment to this Session's full name.
    pub fn child(&self, short: &str) -> Result<Self, InvalidSessionName> {
        if !crate::is_name_segment(short) {
            return Err(InvalidSessionName(short.into()));
        }
        Ok(Self(format!("{self}/{short}")))
    }

    /// Whether this name contains only a root segment.
    pub fn is_root(&self) -> bool {
        !self.0.contains('/')
    }

    /// The final segment, relative to the parent when there is one.
    pub fn short_name(&self) -> &str {
        self.0.rsplit_once('/').map_or(&self.0, |(_, short)| short)
    }

    /// Number of ancestors expressed by this name; roots have depth zero.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a Session name cannot contain more than u32::MAX ancestors"
    )]
    pub fn depth(&self) -> u32 {
        self.0.bytes().filter(|b| *b == b'/').count() as u32
    }

    /// The validated full name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SessionName {
    type Err = InvalidSessionName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if !name.split('/').all(crate::is_name_segment) {
            return Err(InvalidSessionName(name.into()));
        }
        Ok(Self(name.into()))
    }
}

impl Deref for SessionName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for SessionName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for SessionName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<SessionName> for String {
    fn from(name: SessionName) -> Self {
        name.0
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for SessionName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Turn external text into one name segment; return none when no valid segment remains.
pub fn transcribe_name_segment(text: &str) -> Option<String> {
    let mut name = String::new();
    let mut separator = false;
    for c in text.trim().chars() {
        if c.is_whitespace() || c == '/' {
            if !separator {
                name.push('-');
            }
            separator = true;
        } else if !c.is_control() {
            name.push(c);
            separator = false;
        }
    }
    crate::is_name_segment(&name).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_names_preserve_validated_root_and_child_segments() {
        let root = SessionName::root("项目").unwrap();
        assert!(root.is_root());
        let child = root.child("research").unwrap().child("review").unwrap();
        assert_eq!(child.as_str(), "项目/research/review");
        assert_eq!(child.depth(), 2);
        assert_eq!(child.short_name(), "review");
        assert_eq!(
            serde_json::from_str::<SessionName>(&serde_json::to_string(&child).unwrap()).unwrap(),
            child
        );
        for invalid in ["", ".", "..", " main", "main/child", "main\0"] {
            assert!(SessionName::root(invalid).is_err());
            assert!(root.child(invalid).is_err());
        }
        for invalid in ["/main", "main/", "main//child", "main/../child"] {
            assert!(invalid.parse::<SessionName>().is_err());
        }
        let main: SessionName = "main".parse().unwrap();
        let child: SessionName = "main/research".parse().unwrap();
        let sibling: SessionName = "main-2".parse().unwrap();
        assert!(main < child && child < sibling);
    }

    #[test]
    fn external_names_collapse_separators_and_remove_controls() {
        assert_eq!(
            transcribe_name_segment("  项目 /  research\0/ review  ").as_deref(),
            Some("项目-research-review")
        );
        for invalid in ["", "  ", ".", "..", "\0"] {
            assert!(transcribe_name_segment(invalid).is_none());
        }
    }
}
