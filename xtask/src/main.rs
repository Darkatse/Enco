mod docs;

use anyhow::{Context, Result, bail};
use cargo_metadata::DependencyKind;
use std::{path::Path, process::Command};

fn main() -> Result<()> {
    std::env::set_current_dir(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("workspace root")?,
    )?;
    match std::env::args().nth(1).as_deref() {
        Some("boundaries") => boundaries(),
        Some("docs") => docs::generate(std::env::args().any(|a| a == "--check")),
        Some("build-factory") => build_factory(),
        Some("check") => {
            run("cargo", &["fmt", "--all", "--check"])?;
            run(
                "cargo",
                &[
                    "fmt",
                    "--all",
                    "--check",
                    "--manifest-path",
                    "plugins/Cargo.toml",
                ],
            )?;
            boundaries()?;
            lints()?;
            docs::generate(true)?;
            build_factory()?;
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
            run(
                "cargo",
                &[
                    "clippy",
                    "--manifest-path",
                    "plugins/Cargo.toml",
                    "--target",
                    "wasm32-wasip2",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
            run("cargo", &["test", "--workspace"])?;
            run("cargo", &["test", "--manifest-path", "plugins/Cargo.toml"])
        }
        _ => bail!("usage: cargo xtask <check|boundaries|docs [--check]|build-factory>"),
    }
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("{program} {} failed ({status})", args.join(" "));
    }
    Ok(())
}

fn boundaries() -> Result<()> {
    let metadata = cargo_metadata::MetadataCommand::new().no_deps().exec()?;
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
    let plugins = cargo_metadata::MetadataCommand::new()
        .manifest_path("plugins/Cargo.toml")
        .no_deps()
        .exec()?;
    for package in &plugins.packages {
        for dep in &package.dependencies {
            if metadata.packages.iter().any(|p| p.name == dep.name) {
                bail!(
                    "plugin {} depends on host crate {}; plugins use WIT only",
                    package.name,
                    dep.name
                );
            }
        }
    }
    Ok(())
}

/// Both workspaces enforce one lint table, except that plugins deny rather than forbid
/// `unsafe_code`: the Wasm sandbox protects the host, and the generated export glue is
/// accepted with `expect`. The plugin workspace cannot inherit across workspaces, so it
/// keeps a copy, and every member of either workspace must opt in with
/// `[lints] workspace = true`.
fn lints() -> Result<()> {
    fn manifest(path: &str) -> Result<toml::Table> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        toml::from_str(&text).with_context(|| format!("parsing {path}"))
    }
    let workspace_lints = |table: &toml::Table| table.get("workspace")?.get("lints").cloned();
    let mut expected = workspace_lints(&manifest("Cargo.toml")?);
    if let Some(level) = expected
        .as_mut()
        .and_then(|lints| lints.get_mut("rust")?.get_mut("unsafe_code"))
    {
        *level = "deny".into();
    }
    if workspace_lints(&manifest("plugins/Cargo.toml")?) != expected {
        bail!(
            "[workspace.lints] in plugins/Cargo.toml must equal the root table with `unsafe_code = \"deny\"`"
        );
    }
    for root in ["Cargo.toml", "plugins/Cargo.toml"] {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(root)
            .no_deps()
            .exec()?;
        for package in &metadata.packages {
            let member = manifest(package.manifest_path.as_str())?;
            let inherits = member
                .get("lints")
                .and_then(|lints| lints.get("workspace"))
                .and_then(toml::Value::as_bool);
            if inherits != Some(true) {
                bail!(
                    "{} does not inherit the workspace lints; add `[lints] workspace = true`",
                    package.name
                );
            }
        }
    }
    Ok(())
}

fn build_factory() -> Result<()> {
    run(
        "cargo",
        &[
            "build",
            "--manifest-path",
            "plugins/Cargo.toml",
            "--workspace",
            "--target",
            "wasm32-wasip2",
            "--release",
        ],
    )?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path("plugins/Cargo.toml")
        .no_deps()
        .exec()?;
    std::fs::create_dir_all("target/factory")?;
    for name in ["openai-compatible", "deepseek", "typesafe"] {
        let source = metadata
            .target_directory
            .join("wasm32-wasip2/release")
            .join(format!("{}.wasm", name.replace('-', "_")));
        std::fs::copy(&source, format!("target/factory/{name}.wasm"))
            .with_context(|| format!("copying {source}"))?;
    }
    Ok(())
}
