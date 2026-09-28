mod docs;

use anyhow::{Context, Result, bail};
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
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
            run("cargo", &["test", "--workspace"])
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
        let allowed: &[&str] = match package.name.as_str() {
            "enco-core" | "xtask" => &[],
            "enco-kernel" => &["enco-core"],
            "enco-host" | "enco-wasm" => &["enco-core", "enco-kernel"],
            "enco" => &["enco-core", "enco-kernel", "enco-host", "enco-wasm"],
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
    for name in ["provider-openai", "provider-deepseek"] {
        let source = metadata
            .target_directory
            .join("wasm32-wasip2/release")
            .join(format!("{}.wasm", name.replace('-', "_")));
        std::fs::copy(&source, format!("target/factory/{name}.wasm"))
            .with_context(|| format!("copying {source}"))?;
    }
    Ok(())
}
