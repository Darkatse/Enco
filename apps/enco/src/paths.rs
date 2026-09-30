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

    pub fn gitignore(&self) -> PathBuf {
        self.home.join(".gitignore")
    }

    pub fn workspace(&self) -> PathBuf {
        self.home.join("workspace")
    }

    pub fn data(&self) -> PathBuf {
        self.home.join(".data")
    }
    pub fn instructions(&self) -> PathBuf {
        self.home.join("AGENTS.md")
    }
    pub fn db(&self) -> PathBuf {
        self.data().join("enco.db")
    }
    pub fn blobs(&self) -> PathBuf {
        self.data().join("blobs")
    }
    pub fn memory_db(&self) -> PathBuf {
        self.data().join("memory.db")
    }
    pub fn memory_index(&self) -> PathBuf {
        self.data().join("memory-index")
    }

    pub fn socket(&self) -> PathBuf {
        self.data().join("enco.sock")
    }

    pub fn lock(&self) -> PathBuf {
        self.data().join("enco.lock")
    }
}
