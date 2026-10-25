use anyhow::{Context, Result, bail};
use cargo_metadata::{Metadata, Package};
use enco_manual::Rule;
use std::{fs, path::PathBuf};

/// rustdoc JSON is unstable, so `RUSTC_BOOTSTRAP` unlocks it on the pinned toolchain. Builds with
/// and without that variable recompile each other's dependencies, so they keep their own directory.
const TARGET_DIR: &str = "target/rustdoc";

/// Write, or with `check` compare, `API.md` beside each library crate and the SDK, and hold their
/// docs to the rules a machine can check.
pub fn generate(metadata: &[Metadata], check: bool) -> Result<()> {
    let host = &metadata[0];
    let libraries: Vec<&Package> = host
        .workspace_packages()
        .into_iter()
        .filter(|package| {
            package
                .manifest_path
                .strip_prefix(&host.workspace_root)
                .is_ok_and(|path| path.starts_with("crates"))
        })
        .collect();
    rustdoc(
        "Cargo.toml",
        None,
        libraries.iter().map(|package| package.name.as_str()),
    )?;
    let sdk = metadata[1]
        .workspace_packages()
        .into_iter()
        .next()
        .context("SDK package in Cargo metadata")?;
    rustdoc("sdk/Cargo.toml", Some("wasm32-wasip2"), [sdk.name.as_str()])?;
    let mut stale = Vec::new();
    let mut broken = Vec::new();
    for (package, metadata, doc) in libraries
        .iter()
        .map(|package| (*package, host, "doc"))
        .chain([(sdk, &metadata[1], "wasm32-wasip2/doc")])
    {
        let json = PathBuf::from(TARGET_DIR)
            .join(doc)
            .join(format!("{}.json", package.name.replace('-', "_")));
        let krate =
            enco_manual::read(&fs::read(&json).with_context(|| json.display().to_string())?)
                .with_context(|| json.display().to_string())?;
        let directory = package
            .manifest_path
            .parent()
            .context("package directory")?;
        // rustdoc names sources from the workspace root; the manual names them from the crate.
        let sources = match directory.strip_prefix(&metadata.workspace_root) {
            Ok(path) if !path.as_str().is_empty() => format!("{path}/"),
            _ => String::new(),
        };
        let workspace = metadata
            .workspace_root
            .strip_prefix(&host.workspace_root)
            .map(|path| path.join(""))
            .unwrap_or_default();
        for problem in enco_manual::problems(&krate) {
            // The SDK's generated bindings take their docs from the WIT, which the contract gate checks.
            if problem.rule == Rule::Waiver && package.id == sdk.id && problem.item == "bindings" {
                continue;
            }
            broken.push(format!(
                "{}:{}: `{}`: {}",
                workspace.join(problem.file.to_string_lossy().as_ref()),
                problem.line,
                problem.item,
                problem.message
            ));
        }
        let text = enco_manual::api(&krate, &sources);
        let file = directory.join("API.md");
        if check {
            if fs::read_to_string(&file).unwrap_or_default() != text {
                stale.push(file.to_string());
            }
        } else {
            fs::write(&file, text).with_context(|| file.to_string())?;
        }
    }
    if !broken.is_empty() {
        bail!("documentation rules broken:\n{}", broken.join("\n"));
    }
    if !stale.is_empty() {
        bail!("{} out of date; run cargo xtask docs", stale.join(", "));
    }
    Ok(())
}

fn rustdoc<'a>(
    manifest: &str,
    target: Option<&str>,
    packages: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut command = super::command("cargo");
    command
        .args(["doc", "--no-deps", "--lib", "--manifest-path", manifest])
        .args(["--target-dir", TARGET_DIR])
        .env("RUSTC_BOOTSTRAP", "1")
        .env("RUSTDOCFLAGS", enco_manual::RUSTDOCFLAGS);
    if let Some(target) = target {
        command.args(["--target", target]);
    }
    for package in packages {
        command.args(["-p", package]);
    }
    let status = command
        .status()
        .with_context(|| format!("documenting {manifest}"))?;
    if !status.success() {
        bail!("rustdoc for {manifest} failed ({status})");
    }
    Ok(())
}
