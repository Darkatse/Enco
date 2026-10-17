use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

/// A validated directory and routing name for one plugin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PluginName(String);

/// A plugin name outside the public naming grammar.
#[derive(Debug, thiserror::Error)]
#[error("plugin name must use lowercase letters, digits and single hyphens: {0}")]
pub struct InvalidPluginName(String);

impl FromStr for PluginName {
    type Err = InvalidPluginName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if !name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }) {
            return Err(InvalidPluginName(name.into()));
        }
        Ok(Self(name.into()))
    }
}

impl PluginName {
    /// The validated name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for PluginName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Borrow<str> for PluginName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for PluginName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<PluginName> for String {
    fn from(name: PluginName) -> Self {
        name.0
    }
}

impl fmt::Display for PluginName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for PluginName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The owner's configuration file, relative to the Enco home.
pub const CONFIGURATION_FILE: &str = "config.toml";
/// The directory holding one source directory per plugin, relative to the Enco home.
pub const PLUGINS_DIRECTORY: &str = "plugins";

/// An addressable configuration file or plugin directory in the Enco home.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(into = "String")]
pub enum IntentPath {
    /// The owner's complete configuration document.
    Configuration,
    /// Source files under the plugin's current registered name.
    Plugin(PluginName),
}

/// A path that cannot be addressed by the development commands.
#[derive(Debug, thiserror::Error)]
#[error("{0} is not an addressable path; use {CONFIGURATION_FILE} or {PLUGINS_DIRECTORY}/<name>")]
pub struct InvalidIntentPath(String);

impl FromStr for IntentPath {
    type Err = InvalidIntentPath;
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path == CONFIGURATION_FILE {
            return Ok(Self::Configuration);
        }
        path.strip_prefix(PLUGINS_DIRECTORY)
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(|name| name.parse().ok())
            .map(Self::Plugin)
            .ok_or_else(|| InvalidIntentPath(path.into()))
    }
}

impl IntentPath {
    /// Where this path lives under the Enco home.
    pub fn locate(&self, home: &Path) -> PathBuf {
        match self {
            Self::Configuration => home.join(CONFIGURATION_FILE),
            Self::Plugin(name) => home.join(PLUGINS_DIRECTORY).join(name.as_str()),
        }
    }
}

impl fmt::Display for IntentPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration => f.write_str(CONFIGURATION_FILE),
            Self::Plugin(name) => write!(f, "{PLUGINS_DIRECTORY}/{name}"),
        }
    }
}

impl From<IntentPath> for String {
    fn from(path: IntentPath) -> Self {
        path.to_string()
    }
}

impl<'de> Deserialize<'de> for IntentPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_paths_reject_noncanonical_addresses_at_the_json_boundary() {
        for path in [
            "plugins/",
            "plugins/A",
            "plugins/a--b",
            "plugins/../a",
            "plugins/a/file",
            "./config.toml",
            "/config.toml",
        ] {
            assert!(serde_json::from_value::<IntentPath>(serde_json::json!(path)).is_err());
        }
        for path in ["config.toml", "plugins/a-b2"] {
            let parsed: IntentPath = serde_json::from_value(serde_json::json!(path)).unwrap();
            assert_eq!(
                serde_json::to_value(parsed).unwrap(),
                serde_json::json!(path)
            );
        }
    }
}
