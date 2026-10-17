mod docs;
mod testing;

#[path = "../../plugins/factory.rs"]
mod factory;

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

fn workspaces() -> Vec<String> {
    [
        "Cargo.toml",
        "sdk/Cargo.toml",
        "evals/runner/Cargo.toml",
        "plugins/test-fixture/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(factory::PLUGINS.map(|(name, _)| format!("plugins/{name}/Cargo.toml")))
    .collect()
}

fn main() -> Result<()> {
    std::env::set_current_dir(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("workspace root")?,
    )?;
    match std::env::args().nth(1).as_deref() {
        Some("boundaries") => boundaries(&metadata()?),
        Some("docs") => docs::generate(std::env::args().any(|a| a == "--check")),
        Some("build-factory") => build_factory(),
        Some("check") => {
            in_workspaces(|manifest| {
                run(
                    "cargo",
                    &["fmt", "--all", "--check", "--manifest-path", &manifest],
                )
            })?;
            let metadata = metadata()?;
            boundaries(&metadata)?;
            lints(&metadata)?;
            file_lengths(&metadata)?;
            docs::generate(true)?;
            build_factory()?;
            testing::run(&metadata)
        }
        _ => bail!("usage: cargo xtask <check|boundaries|docs [--check]|build-factory>"),
    }
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = command(program)
        .args(args)
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("{program} {} failed ({status})", args.join(" "));
    }
    Ok(())
}

/// Compiler commands and tests which invoke Cargo share the gate's offline policy.
fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env("CARGO_NET_OFFLINE", "true");
    command
}

fn metadata() -> Result<Vec<Metadata>> {
    in_workspaces(|manifest| {
        let output = command("cargo")
            .args([
                "metadata",
                "--format-version=1",
                "--no-deps",
                "--manifest-path",
                &manifest,
            ])
            .output()
            .with_context(|| format!("reading metadata for {manifest}"))?;
        if !output.status.success() {
            bail!("{manifest}: {}", String::from_utf8_lossy(&output.stderr));
        }
        serde_json::from_slice(&output.stdout).map_err(Into::into)
    })
}

/// Workspace checks are independent; preserve manifest order in their results.
fn in_workspaces<T: Send>(operation: impl Fn(String) -> Result<T> + Sync) -> Result<Vec<T>> {
    std::thread::scope(|scope| {
        let operation = &operation;
        let checks: Vec<_> = workspaces()
            .into_iter()
            .map(|manifest| scope.spawn(move || operation(manifest)))
            .collect();
        checks
            .into_iter()
            .map(|check| {
                check
                    .join()
                    .map_err(|_| anyhow::anyhow!("workspace check panicked"))?
            })
            .collect()
    })
}

fn boundaries(workspace_metadata: &[Metadata]) -> Result<()> {
    let metadata = &workspace_metadata[0];
    for package in &metadata.packages {
        // Allowed workspace dependencies, and whether the crate is an application boundary,
        // the only place `anyhow` may appear; library crates define errors with thiserror.
        let (allowed, application): (&[&str], bool) = match package.name.as_str() {
            "enco-core" => (&[], false),
            "xtask" => (&[], true),
            "enco-kernel" => (&["enco-core"], false),
            "enco-host" | "enco-wasm" => (&["enco-core", "enco-kernel"], false),
            "enco" => (
                &["enco-core", "enco-kernel", "enco-host", "enco-wasm"],
                true,
            ),
            name => bail!("unassigned workspace member: {name}"),
        };
        for dep in &package.dependencies {
            if metadata.packages.iter().any(|p| p.name == dep.name)
                && !allowed.contains(&dep.name.as_str())
            {
                bail!(
                    "{} depends on {}; allowed workspace dependencies: {allowed:?}",
                    package.name,
                    dep.name
                );
            }
            if dep.name == "anyhow" && dep.kind == DependencyKind::Normal && !application {
                bail!(
                    "{} depends on anyhow; library crates define errors with thiserror",
                    package.name
                );
            }
        }
    }
    for (manifest, workspace) in workspaces().iter().zip(workspace_metadata).skip(1) {
        let boundary = if manifest.starts_with("evals/") {
            "the evaluator uses the CLI only"
        } else {
            "plugins and SDK use WIT only"
        };
        for package in &workspace.packages {
            for dep in &package.dependencies {
                if metadata.packages.iter().any(|p| p.name == dep.name) {
                    bail!(
                        "{} depends on host crate {}; {boundary}",
                        package.name,
                        dep.name
                    );
                }
            }
        }
    }
    Ok(())
}

/// Every independently built plugin carries the same lint policy as the host, with ABI glue allowed.
fn lints(workspace_metadata: &[Metadata]) -> Result<()> {
    fn manifest(path: &str) -> Result<toml::Table> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        toml::from_str(&text).with_context(|| format!("parsing {path}"))
    }
    let root = manifest("Cargo.toml")?;
    let expected = root["workspace"]["lints"].clone();
    for (path, metadata) in workspaces().iter().zip(workspace_metadata) {
        let table = manifest(path)?;
        let independent_plugin = path.starts_with("plugins/") || path.starts_with("sdk/");
        let mut wanted = expected.clone();
        if independent_plugin {
            wanted["rust"]["unsafe_code"] = "deny".into();
        }
        let actual = if independent_plugin {
            table.get("lints")
        } else {
            table.get("workspace").and_then(|v| v.get("lints"))
        };
        if actual != Some(&wanted) {
            bail!("{path}: lint table differs from the root policy");
        }
        if !independent_plugin {
            for package in &metadata.packages {
                let member = manifest(package.manifest_path.as_str())?;
                if member
                    .get("lints")
                    .and_then(|v| v.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    != Some(true)
                {
                    bail!("{} must inherit workspace lints", package.name);
                }
            }
        }
    }
    Ok(())
}

/// A reader, person or agent, takes in a file whole, and a longer one usually answers two
/// questions. Integration tests read as scenarios, and each test file links a binary of its own.
const SOURCE_LINES: usize = 500;
const TEST_LINES: usize = 1200;

fn file_lengths(workspace_metadata: &[Metadata]) -> Result<()> {
    let root = std::env::current_dir()?;
    let mut long = Vec::new();
    for metadata in workspace_metadata {
        for package in &metadata.packages {
            let dir = package
                .manifest_path
                .parent()
                .context("package directory")?
                .as_std_path();
            for path in rust_files(dir)? {
                let limit = if path.strip_prefix(dir)?.starts_with("tests") {
                    TEST_LINES
                } else {
                    SOURCE_LINES
                };
                let lines = std::fs::read_to_string(&path)?.lines().count();
                if lines > limit {
                    let path = path.strip_prefix(&root).unwrap_or(&path).display();
                    long.push(format!("{path}: {lines} lines (limit {limit})"));
                }
            }
        }
    }
    long.sort();
    if !long.is_empty() {
        bail!("split these files by responsibility:\n{}", long.join("\n"));
    }
    Ok(())
}

fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                files.extend(rust_files(&path)?);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(files)
}

fn build_factory() -> Result<()> {
    std::fs::create_dir_all("target/factory")?;
    for (name, _) in factory::PLUGINS {
        run(
            "cargo",
            &[
                "build",
                "--manifest-path",
                &format!("plugins/{name}/Cargo.toml"),
                "--target",
                "wasm32-wasip2",
                "--release",
                "--target-dir",
                "target/plugin-build",
            ],
        )?;
        let source = format!(
            "target/plugin-build/wasm32-wasip2/release/{}.wasm",
            name.replace('-', "_")
        );
        let destination = format!("target/factory/{name}.wasm");
        let bytes = std::fs::read(&source).with_context(|| format!("reading {source}"))?;
        let unchanged = match std::fs::read(&destination) {
            Ok(previous) => previous == bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error).with_context(|| format!("reading {destination}")),
        };
        // Identical embedded bytes must not force every application test executable to relink.
        if !unchanged {
            std::fs::write(&destination, bytes)
                .with_context(|| format!("writing {destination}"))?;
        }
    }
    Ok(())
}

fn clippy() -> Result<()> {
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    for manifest in workspaces()
        .into_iter()
        .filter(|path| path.starts_with("sdk/") || path.starts_with("plugins/"))
    {
        run(
            "cargo",
            &[
                "clippy",
                "--manifest-path",
                &manifest,
                "--target",
                "wasm32-wasip2",
                "--target-dir",
                "target/plugin-build",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        )?;
    }
    run(
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            "evals/runner/Cargo.toml",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    Ok(())
}
