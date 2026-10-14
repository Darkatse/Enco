mod docs;

use anyhow::{Context, Result, bail};
use cargo_metadata::DependencyKind;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

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
            for manifest in [
                "Cargo.toml",
                "plugins/Cargo.toml",
                "evals/runner/Cargo.toml",
            ] {
                run(
                    "cargo",
                    &["fmt", "--all", "--check", "--manifest-path", manifest],
                )?;
            }
            boundaries()?;
            lints()?;
            file_lengths()?;
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
            // The evaluation tests drive the enco binary, so it must be built before they start.
            run("cargo", &["test", "--workspace", "--no-run"])?;
            tests()
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

fn tests() -> Result<()> {
    let metadata = cargo_metadata::MetadataCommand::new().no_deps().exec()?;
    let app = metadata
        .packages
        .iter()
        .find(|package| package.name == "enco")
        .context("application package")?;
    // Cargo runs test executables sequentially; two groups overlap independent application targets.
    let mut application = [Command::new("cargo"), Command::new("cargo")];
    for command in &mut application {
        command.args(["test", "-p", "enco"]);
    }
    application[0].arg("--bins");
    for (index, target) in app
        .targets
        .iter()
        .filter(|target| target.is_test())
        .enumerate()
    {
        let group = index % application.len();
        application[group].args(["--test", &target.name]);
    }
    let [first, second] = application;
    let mut workspace = Command::new("cargo");
    workspace.args(["test", "--workspace", "--exclude", "enco"]);
    let mut plugins = Command::new("cargo");
    plugins.args(["test", "--manifest-path", "plugins/Cargo.toml"]);
    let mut evaluation = Command::new("cargo");
    evaluation
        .args(["test", "--manifest-path", "evals/runner/Cargo.toml"])
        .env("ENCO_BIN", metadata.target_directory.join("debug/enco"));
    concurrently([
        ("application tests (1/2)", first),
        ("application tests (2/2)", second),
        ("workspace tests", workspace),
        ("plugin tests", plugins),
        ("evaluation tests", evaluation),
    ])
}

/// Run each command to completion on its own thread, then print the outputs in order.
fn concurrently<const N: usize>(commands: [(&str, Command); N]) -> Result<()> {
    let outputs = std::thread::scope(|scope| {
        commands
            .map(|(name, command)| (name, scope.spawn(move || interleaved(command))))
            .map(|(name, handle)| (name, handle.join()))
    });
    let mut failed = Vec::new();
    for (name, output) in outputs {
        let (status, output) = output
            .map_err(|_| anyhow::anyhow!("{name}: thread panicked"))?
            .with_context(|| format!("running {name}"))?;
        std::io::stdout().write_all(&output)?;
        if !status.success() {
            failed.push(name);
        }
    }
    if !failed.is_empty() {
        bail!("{} failed", failed.join(", "));
    }
    Ok(())
}

/// One pipe for both streams keeps cargo's progress next to the results it introduces.
fn interleaved(mut command: Command) -> std::io::Result<(ExitStatus, Vec<u8>)> {
    let (mut reader, writer) = std::io::pipe()?;
    let mut child = command.stdout(writer.try_clone()?).stderr(writer).spawn()?;
    // Our copies of the write end must close, or the read below never reaches the end.
    drop(command);
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok((child.wait()?, output))
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
    for (manifest, boundary) in [
        ("plugins/Cargo.toml", "plugins use WIT only"),
        ("evals/runner/Cargo.toml", "the evaluator uses the CLI only"),
    ] {
        let workspace = cargo_metadata::MetadataCommand::new()
            .manifest_path(manifest)
            .no_deps()
            .exec()?;
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

/// All workspaces enforce one lint table, except that plugins deny rather than forbid
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
    if workspace_lints(&manifest("evals/runner/Cargo.toml")?) != expected {
        bail!("[workspace.lints] in evals/runner/Cargo.toml must equal the root table");
    }
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
    for root in [
        "Cargo.toml",
        "plugins/Cargo.toml",
        "evals/runner/Cargo.toml",
    ] {
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

/// A reader, person or agent, takes in a file whole, and a longer one usually answers two
/// questions. Integration tests read as scenarios, and each test file links a binary of its own.
const SOURCE_LINES: usize = 500;
const TEST_LINES: usize = 1200;

fn file_lengths() -> Result<()> {
    let root = std::env::current_dir()?;
    let mut long = Vec::new();
    for manifest in [
        "Cargo.toml",
        "plugins/Cargo.toml",
        "evals/runner/Cargo.toml",
    ] {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(manifest)
            .no_deps()
            .exec()?;
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
    for name in ["openai-compatible", "deepseek", "typesafe", "web"] {
        let source = metadata
            .target_directory
            .join("wasm32-wasip2/release")
            .join(format!("{}.wasm", name.replace('-', "_")));
        std::fs::copy(&source, format!("target/factory/{name}.wasm"))
            .with_context(|| format!("copying {source}"))?;
    }
    Ok(())
}
