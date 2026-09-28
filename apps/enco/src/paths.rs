use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let path = std::env::var_os("ENCO_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|p| p.join(".enco")))
            .context("cannot determine home directory; set ENCO_HOME")?;
        Ok(Self {
            home: std::path::absolute(path)?,
        })
    }

    pub fn config(&self) -> PathBuf {
        self.home.join("config.toml")
    }

    pub fn workspace(&self) -> PathBuf {
        self.home.join("workspace")
    }

    pub fn socket(&self) -> PathBuf {
        self.home.join("enco.sock")
    }
}
