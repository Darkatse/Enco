use super::{
    Development, DevelopmentError, check_cancelled,
    intent::{Change, Prepared},
};
use enco_kernel::{Registered, Registry, validate_plugin_name};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

impl Development {
    /// Write a standalone tool plugin without registering an identity or changing running code.
    pub async fn scaffold(
        &self,
        registry: &Registry,
        name: &str,
    ) -> Result<Vec<PathBuf>, DevelopmentError> {
        let _operation = self.operation.lock().await;
        validate_plugin_name(name)?;
        let registered = registry.registered_intent().await;
        if registered.plugins.contains_key(name) {
            return Err(DevelopmentError::Invalid(format!(
                "{name}: name is already registered or reserved"
            )));
        }
        let root = self.home.join("plugins").join(name);
        let manifest = format!(
            "{}\n{}",
            MANIFEST.replace("__NAME__", name),
            self.plugin_lints()?
        );
        // Even the first build uses the SDK's tested versions.
        let lock = self
            .sdk
            .lock()
            .ok_or_else(|| DevelopmentError::Invalid("embedded SDK has no Cargo.lock".into()))?;
        let files: [(&str, Vec<u8>); 5] = [
            ("Cargo.toml", manifest.into_bytes()),
            ("Cargo.lock", lock.to_vec()),
            (".cargo/config.toml", CARGO_CONFIG.as_bytes().to_vec()),
            (
                "src/lib.rs",
                include_str!("scaffold/lib.rs.in")
                    .replace("__NAME__", name)
                    .into_bytes(),
            ),
            ("README.md", README.replace("__NAME__", name).into_bytes()),
        ];
        tokio::fs::create_dir_all(self.home.join("plugins")).await?;
        tokio::fs::create_dir(&root)
            .await
            .map_err(|error| DevelopmentError::Input {
                path: root.clone(),
                reason: error.to_string(),
            })?;
        let mut written = Vec::new();
        for (path, content) in files {
            let path = root.join(path);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&path, content).await?;
            written.push(path);
        }
        Ok(written)
    }

    /// Move the source directory and register its new name, preserving identity and content.
    pub async fn rename(
        &self,
        registry: &Registry,
        name: &str,
        to: &str,
        cancel: &CancellationToken,
    ) -> Result<Registered, DevelopmentError> {
        let _operation = self.operation.lock().await;
        check_cancelled(cancel)?;
        self.git.check().await?;
        registry.check_rename(name, to).await?;
        let source = format!("plugins/{name}");
        let destination = format!("plugins/{to}");
        if tokio::fs::try_exists(self.home.join(&destination)).await? {
            return Err(DevelopmentError::Invalid(format!(
                "{destination} already exists"
            )));
        }
        let baseline = registry.registered_intent().await;
        let revision = &baseline
            .base
            .as_ref()
            .ok_or_else(|| DevelopmentError::Invalid("no registered intent".into()))?
            .revision;
        let directories = self.git.directories(revision).await?;
        let tree = directories
            .get(name)
            .ok_or_else(|| {
                DevelopmentError::Invalid(format!("{name}: no registered source directory"))
            })?
            .clone();
        tokio::fs::rename(self.home.join(&source), self.home.join(&destination)).await?;
        let prepared = Prepared {
            plugins: [(
                name.into(),
                Change::Rename {
                    to: to.into(),
                    tree,
                },
            )]
            .into(),
            ..Prepared::default()
        };
        self.register(registry, &baseline, prepared, "rename", cancel)
            .await
            .map_err(|error| DevelopmentError::Rename {
                from: source,
                to: destination,
                source: Box::new(error),
            })
    }

    fn plugin_lints(&self) -> Result<String, DevelopmentError> {
        let bytes = self
            .sdk
            .file("Cargo.toml")
            .ok_or_else(|| DevelopmentError::Invalid("embedded SDK has no Cargo.toml".into()))?;
        let text = std::str::from_utf8(bytes)
            .map_err(|error| DevelopmentError::Invalid(error.to_string()))?;
        let manifest: toml::Value = toml::from_str(text)
            .map_err(|error: toml::de::Error| DevelopmentError::Invalid(error.to_string()))?;
        let lints = manifest
            .get("lints")
            .ok_or_else(|| DevelopmentError::Invalid("embedded SDK has no lint table".into()))?;
        toml::to_string(&std::collections::BTreeMap::from([("lints", lints)]))
            .map_err(|error| DevelopmentError::Invalid(error.to_string()))
    }
}

const CARGO_CONFIG: &str = "[build]\ntarget = \"wasm32-wasip2\"\n";

const README: &str = "# __NAME__\n\nEdit src/lib.rs, then use self_apply to build and register this plugin.\n\n<!-- enco:generated:begin -->\n<!-- enco:generated:end -->\n";

const MANIFEST: &str = r#"[package]
name = "plugin-__NAME__"
version = "0.1.0"
edition = "2024"

[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
enco-sdk = { path = "../../sdk" }

[profile.release]
opt-level = "s"
lto = true
strip = true
"#;
