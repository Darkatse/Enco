//! Creating a plugin directory in the home, from a factory plugin's shipped source or from the
//! tool template; nothing is registered.
use super::{
    Development, DevelopmentError, Failed, Operation,
    tree::{Directory, FileTree, Ownership},
};
use enco_core::{CodeIntent, IntentPath, PluginName};
use enco_kernel::{RegisteredSource, Registry};
use std::{collections::BTreeMap, path::PathBuf};

/// One factory plugin's shipped files, as paths relative to its directory and their bytes.
pub type FactoryFiles = [(&'static str, &'static [u8])];

/// The source of each factory plugin, as the host ships it.
#[derive(Default)]
pub struct FactorySources(BTreeMap<PluginName, FileTree>);

impl FactorySources {
    /// Read the embedded sources, keyed by the factory plugin's name.
    pub fn new(sources: &[(&str, &FactoryFiles)]) -> Result<Self, DevelopmentError> {
        sources
            .iter()
            .map(|(name, files)| {
                let name = name
                    .parse()
                    .map_err(|error| DevelopmentError::Host(format!("factory source: {error}")))?;
                let tree = FileTree::from_files(
                    files.iter().map(|(path, bytes)| (*path, bytes.to_vec())),
                )?;
                Ok((name, tree))
            })
            .collect::<Result<_, DevelopmentError>>()
            .map(Self)
    }
}

impl Development {
    /// Write a plugin directory without registering an identity or changing running code.
    ///
    /// A factory plugin that follows the host starts from its shipped source: from the next
    /// registration it is ordinary source and no longer follows. Any other free name starts from
    /// the tool template.
    pub async fn scaffold(
        &self,
        registry: &Registry,
        name: &PluginName,
    ) -> Result<Vec<PathBuf>, Failed> {
        let _operation = self.operation.lock().await;
        self.create(registry, name)
            .await
            .map_err(|error| Operation::Scaffold.failed(error))
    }

    async fn create(
        &self,
        registry: &Registry,
        name: &PluginName,
    ) -> Result<Vec<PathBuf>, DevelopmentError> {
        let registered = registry.registered_intent(None).await?.plugins.remove(name);
        let follows = registered.as_ref().is_none_or(|plugin| {
            !matches!(
                plugin.code,
                Some(RegisteredSource {
                    intent: CodeIntent::Directory { .. },
                    ..
                })
            )
        });
        let tree = match self.factory.0.get(name) {
            Some(source) if follows => source.clone(),
            _ if registered.is_some() => {
                return Err(DevelopmentError::NameTaken { name: name.clone() });
            }
            _ => self.template(name)?,
        };
        let root = IntentPath::Plugin(name.clone()).locate(&self.home);
        let directory = Directory::read(&root, Ownership::Source).await?;
        if directory.exists() {
            return Err(DevelopmentError::DirectoryExists { name: name.clone() });
        }
        let plan =
            directory
                .plan_write(tree)
                .map_err(|conflicts| DevelopmentError::WriteConflicts {
                    path: root,
                    conflicts,
                })?;
        let mut written = Vec::new();
        match plan.execute(&mut written).await {
            Ok(()) => Ok(written),
            Err(source) => Err(DevelopmentError::Partial {
                written,
                source: Box::new(source),
            }),
        }
    }

    fn template(&self, name: &PluginName) -> Result<FileTree, DevelopmentError> {
        let manifest = self.manifest(name)?;
        // Even the first build uses the SDK's tested versions.
        let lock = self
            .sdk
            .lock()
            .ok_or_else(|| DevelopmentError::Host("embedded SDK has no Cargo.lock".into()))?;
        FileTree::from_files([
            ("Cargo.toml", manifest.into_bytes()),
            (".gitignore", b"/target/\n.DS_Store\n".to_vec()),
            ("Cargo.lock", lock.to_vec()),
            (".cargo/config.toml", CARGO_CONFIG.as_bytes().to_vec()),
            (
                "src/lib.rs",
                include_str!("scaffold/lib.rs.in")
                    .replace("__NAME__", name)
                    .into_bytes(),
            ),
            (
                super::readme::FILE,
                README
                    .replace("__NAME__", name)
                    .replace("__APPLY__", super::APPLY)
                    .into_bytes(),
            ),
        ])
    }

    /// The new plugin's `Cargo.toml`, built from a template and the SDK's manifest.
    ///
    /// Its lint table and argument libraries are copied from the SDK's manifest: argument types
    /// must derive the traits that the SDK's functions take, and the first build resolves them
    /// from the SDK's lock.
    fn manifest(&self, name: &PluginName) -> Result<String, DevelopmentError> {
        let host = |error: &dyn std::fmt::Display| {
            DevelopmentError::Host(format!("embedded SDK manifest: {error}"))
        };
        let bytes = self
            .sdk
            .file("Cargo.toml")
            .ok_or_else(|| host(&"missing"))?;
        let text = std::str::from_utf8(bytes).map_err(|error| host(&error))?;
        let sdk: toml::Value = toml::from_str(text).map_err(|error| host(&error))?;
        let mut dependencies = String::new();
        for library in ARGUMENT_LIBRARIES {
            let requirement = sdk
                .get("dependencies")
                .and_then(|table| table.get(library))
                .ok_or_else(|| host(&format!("no `{library}` dependency")))?;
            dependencies.push_str(&format!("{library} = {requirement}\n"));
        }
        let lints = sdk.get("lints").ok_or_else(|| host(&"no lint table"))?;
        let lints = toml::to_string(&std::collections::BTreeMap::from([("lints", lints)]))
            .map_err(|error| host(&error))?;
        Ok(format!(
            "{}\n{lints}",
            MANIFEST
                .replace("__DEPENDENCIES__", &dependencies)
                .replace("__NAME__", name)
        ))
    }
}

/// Libraries a plugin uses to declare and parse typed tool arguments and to return JSON.
const ARGUMENT_LIBRARIES: [&str; 3] = ["schemars", "serde", "serde_json"];

const CARGO_CONFIG: &str = "[build]\ntarget = \"wasm32-wasip2\"\n";

const README: &str = "# __NAME__\n\nEdit src/lib.rs, then use __APPLY__ to build and register this plugin.\n\n<!-- enco:generated:begin -->\n<!-- enco:generated:end -->\n";

const MANIFEST: &str = r#"[package]
name = "plugin-__NAME__"
version = "0.1.0"
edition = "2024"

[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
enco-sdk = { path = "../../sdk" }
__DEPENDENCIES__
[profile.release]
opt-level = "s"
lto = true
strip = true
"#;
