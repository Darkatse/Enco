use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "../../plugins/factory.rs"]
mod factory;

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    embed_factory(&root)?;
    embed_sdk(&root)?;
    record_revision(&root)?;
    Ok(())
}

fn embed_factory(root: &Path) -> Result<(), Box<dyn Error>> {
    let mut source = String::from("const FACTORY: &[(&str, &str, &[u8])] = &[\n");
    for (name, id) in factory::PLUGINS {
        let path = root.join(format!("target/factory/{name}.wasm"));
        if !path.try_exists()? {
            return Err(format!(
                "{}: factory artifact missing; run `cargo xtask build-factory` first",
                path.display()
            )
            .into());
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let key = name.replace('-', "_").to_uppercase();
        println!("cargo:rustc-env=ENCO_FACTORY_{key}={}", path.display());
        source.push_str(&format!(
            "({name:?}, {id:?}, include_bytes!({:?})),\n",
            path.to_str().ok_or("factory path is not UTF-8")?
        ));
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?)
            .join("embedded_factory.rs"),
        source,
    )?;
    Ok(())
}

fn embed_sdk(root: &Path) -> Result<(), Box<dyn Error>> {
    let sdk = root.join("sdk");
    let mut sources = vec![sdk.join("Cargo.toml"), sdk.join("Cargo.lock")];
    collect_files(&sdk.join("src"), &mut sources)?;
    collect_files(&sdk.join("wit"), &mut sources)?;
    let mut files = sources
        .into_iter()
        .map(|path| Ok((path.strip_prefix(&sdk)?.to_owned(), path)))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    // Home builds use the toolchain that builds the host; the SDK hash covers it.
    let toolchain = root.join("rust-toolchain.toml");
    files.push(("rust-toolchain.toml".into(), toolchain.clone()));
    files.sort();
    let mut source = String::from("pub(crate) const SDK_FILES: &[(&str, &[u8])] = &[\n");
    for (relative, path) in files {
        let relative = relative.to_str().ok_or("SDK path is not UTF-8")?;
        source.push_str(&format!(
            "({relative:?}, include_bytes!({:?})),\n",
            path.to_str().ok_or("SDK path is not UTF-8")?
        ));
    }
    source.push_str("];\npub(crate) fn sdk() -> Result<enco_host::development::Sdk, enco_host::development::DevelopmentError> { enco_host::development::Sdk::new(SDK_FILES) }\n");
    for input in ["Cargo.toml", "Cargo.lock", "src", "wit"] {
        println!("cargo:rerun-if-changed={}", sdk.join(input).display());
    }
    println!("cargo:rerun-if-changed={}", toolchain.display());
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?).join("sdk.rs"),
        source,
    )?;
    Ok(())
}

/// Only source and contract files enter the bundle; editor and system files would change its hash.
fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| ["rs", "wit", "md"].iter().any(|kind| extension == *kind))
            && !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
            files.push(path);
        }
    }
    Ok(())
}

/// The version names the HEAD commit only. A dirty-tree flag would make every edit relink the
/// application and its tests, so development builds may embed uncommitted changes.
fn record_revision(root: &Path) -> Result<(), Box<dyn Error>> {
    let revision = git(root, &["rev-parse", "HEAD"])?;
    println!("cargo:rustc-env=ENCO_REVISION={revision}");
    println!(
        "cargo:rustc-env=ENCO_VERSION={} ({revision})",
        env!("CARGO_PKG_VERSION")
    );
    let reference = git(root, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
    for input in ["HEAD", &reference] {
        let mut path = root.join(git(root, &["rev-parse", "--git-path", input])?);
        // A packed branch acquires a loose reference on its next commit.
        while !path.try_exists()? {
            path = path
                .parent()
                .ok_or("Git reference has no parent")?
                .to_owned();
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let packed = root.join(git(root, &["rev-parse", "--git-path", "packed-refs"])?);
    if packed.try_exists()? {
        println!("cargo:rerun-if-changed={}", packed.display());
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
