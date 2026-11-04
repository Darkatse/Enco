//! `cargo xtask prune` deletes compiled outputs no gate build uses, so the target directories
//! shrink and the next gate stays warm. Cargo names the gate's units as prune replays its builds,
//! doctests included; outputs only other builds use, like `cargo run -p enco`'s, are rebuilt on
//! demand. Run it while nothing else builds.

use anyhow::{Context, Result, bail};
use cargo_metadata::{Message, Metadata};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

/// Warm Cargo caches of the application tests' source scenarios; see their `build_target`.
const SCENARIOS: &str = "target/plugin-tests";
const BAR: usize = 40;

/// Delete, or with `dry_run` only measure, what the gate no longer uses, and report its size.
pub fn run(metadata: &[Metadata], dry_run: bool) -> Result<()> {
    let live = live(metadata)?;
    // Absolute, as Cargo reports the outputs it hard-links out of `deps`.
    let root = std::env::current_dir()?;
    let scenarios = root.join(SCENARIOS);
    let mut doomed = Vec::new();
    for profile in profiles(&root.join("target"), 3)?
        .into_iter()
        .chain(profiles(&root.join("evals/runner/target"), 2)?)
        .filter(|profile| !profile.starts_with(&scenarios))
    {
        stale(&profile, &live, &mut doomed)?;
    }
    let temporary = temporary()?;
    for scenario in entries(&scenarios)? {
        if is_directory(&scenario)? {
            abandoned(&scenario, &temporary, &mut doomed)?;
        }
    }
    let (files, bytes) = remove(&doomed, dry_run)?;
    let summary = format!("{files} files, {} total", human(bytes));
    if dry_run {
        status("Summary", &format!("{summary}; --dry-run removed nothing"))
    } else {
        status("Removed", &summary)
    }
}

#[derive(Default)]
struct Live {
    /// The hash Cargo appends to every compiled file of a unit the gate builds.
    units: HashSet<String>,
    /// Outputs Cargo reports by a plain name: binaries and cdylibs it hard-links out of `deps`.
    /// Keyed with `_` for `-`, since `deps` names a binary by its crate name.
    uplifted: HashMap<PathBuf, PathBuf>,
    /// Crate names, which name incremental directories where a unit hash would.
    crates: HashSet<String>,
}

/// Replay the gate's builds and collect every unit Cargo reports, fresh or rebuilt.
fn live(metadata: &[Metadata]) -> Result<Live> {
    // `cargo xtask` first builds this binary, whose dependencies no other gate build shares.
    let mut xtask = super::command("cargo");
    xtask.args(["build", "--package", "xtask"]);
    let mut builds = vec![xtask];
    builds.extend(super::testing::builds(metadata));
    builds.extend(super::clippy_builds());
    builds.extend(super::factory::PLUGINS.map(|(name, _)| super::factory_build(name)));
    builds.extend(super::manual::builds(metadata)?);
    let mut live = Live::default();
    let mut progress = Progress::new("Inspecting", builds.len());
    for (index, build) in builds.iter().enumerate() {
        let line = super::command_line(build);
        let output = json(build)
            .output()
            .with_context(|| format!("running {line}"))?;
        if !output.status.success() {
            progress.finish()?;
            std::io::stderr().write_all(&output.stderr)?;
            bail!("{line} failed; prune keeps only what passing builds use");
        }
        for message in Message::parse_stream(output.stdout.as_slice()) {
            if let Message::CompilerArtifact(artifact) = message? {
                live.crates.insert(artifact.target.name.replace('-', "_"));
                for path in artifact.filenames {
                    if let Some(hash) = path.file_name().and_then(unit_hash) {
                        live.units.insert(hash.to_owned());
                    } else if let Some(name) = path.file_name() {
                        let key = path.with_file_name(name.replace('-', "_"));
                        live.uplifted.insert(key.into(), path.into());
                    }
                }
            }
        }
        progress.tick(index + 1)?;
    }
    progress.finish()?;
    Ok(live)
}

/// The same build reporting its units as JSON. The flag goes before `--`, after which Cargo
/// passes arguments on to the compiler.
fn json(build: &Command) -> Command {
    let args: Vec<&OsStr> = build.get_args().collect();
    let split = args
        .iter()
        .position(|arg| *arg == "--")
        .unwrap_or(args.len());
    let mut json = Command::new(build.get_program());
    json.args(&args[..split])
        .arg("--message-format=json")
        .args(&args[split..]);
    for (key, value) in build.get_envs() {
        match value {
            Some(value) => json.env(key, value),
            None => json.env_remove(key),
        };
    }
    json
}

/// Compiled files of units the gate no longer builds, and incremental state of crates it no
/// longer compiles. Fingerprints stay: they are small, and rustdoc's units report no hash.
fn stale(profile: &Path, live: &Live, doomed: &mut Vec<PathBuf>) -> Result<()> {
    let deps = entries(&profile.join("deps"))?;
    let mut linked = HashSet::new();
    for path in &deps {
        if let Some(hash) = uplifted(path, profile, live)? {
            linked.insert(hash.to_owned());
        }
    }
    doomed.extend(deps.into_iter().filter(|path| {
        name(path)
            .and_then(unit_hash)
            .is_some_and(|hash| !live.units.contains(hash) && !linked.contains(hash))
    }));
    for path in entries(&profile.join("incremental"))? {
        if name(&path)
            .and_then(|name| name.rsplit_once('-'))
            .is_some_and(|(krate, _)| !live.crates.contains(krate))
        {
            doomed.push(path);
        }
    }
    Ok(())
}

/// The hash of a `deps` file that is the same file as an output Cargo reported by its plain name.
fn uplifted<'a>(path: &'a Path, profile: &Path, live: &Live) -> Result<Option<&'a str>> {
    let Some((name, hash)) = name(path).and_then(|name| Some((name, unit_hash(name)?))) else {
        return Ok(None);
    };
    let plain = profile.join(name.replacen(&format!("-{hash}"), "", 1).replace('-', "_"));
    let Some(reported) = live.uplifted.get(&plain) else {
        return Ok(None);
    };
    // A hard link shares its size and modification time; a stale build's output does not.
    let (linked, reported) = (fs::metadata(path)?, fs::metadata(reported)?);
    Ok(
        (linked.len() == reported.len() && linked.modified()? == reported.modified()?)
            .then_some(hash),
    )
}

/// Units whose crate root lies in a test's temporary home, which the next run builds from a new
/// path, and the build logs of past runs. A dep-info file names its outputs through that home
/// too, so only its first source decides.
fn abandoned(scenario: &Path, temporary: &[String], doomed: &mut Vec<PathBuf>) -> Result<()> {
    for profile in profiles(scenario, 2)? {
        let deps = entries(&profile.join("deps"))?;
        let mut dead = HashSet::new();
        for path in deps
            .iter()
            .filter(|path| path.extension() == Some(OsStr::new("d")))
        {
            let info = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let info = String::from_utf8_lossy(&info);
            let root = info
                .lines()
                .next()
                .and_then(|line| line.split_once(": "))
                .and_then(|(_, sources)| sources.split_whitespace().next());
            if root.is_some_and(|root| temporary.iter().any(|prefix| root.starts_with(prefix))) {
                dead.extend(name(path).and_then(unit_hash).map(str::to_owned));
            }
        }
        doomed.extend(deps.into_iter().filter(|path| {
            name(path)
                .and_then(unit_hash)
                .is_some_and(|hash| dead.contains(hash))
        }));
    }
    doomed.extend(entries(&scenario.join("logs"))?);
    Ok(())
}

/// The system temporary directory as tests and their daemons may spell it.
fn temporary() -> Result<Vec<String>> {
    let spelled = std::env::temp_dir();
    let resolved = spelled
        .canonicalize()
        .context("resolving the temporary directory")?;
    Ok([spelled, resolved]
        .iter()
        .map(|path| path.join("").to_string_lossy().into_owned())
        .collect())
}

/// The 16 hexadecimal digits Cargo appends to every compiled file of a unit, as in
/// `libserde-0123456789abcdef.rlib` or `serde-0123456789abcdef.0abc.rcgu.o`.
fn unit_hash(name: &str) -> Option<&str> {
    let (_, hash) = name.split('.').next()?.rsplit_once('-')?;
    (hash.len() == 16
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
    .then_some(hash)
}

/// Cargo profile directories, such as `debug` or `wasm32-wasip2/release`, at most `depth` below.
fn profiles(dir: &Path, depth: usize) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if depth == 0 {
        return Ok(found);
    }
    for path in entries(dir)? {
        if !is_directory(&path)? {
            continue;
        }
        if path.join(".fingerprint").is_dir() {
            found.push(path);
        } else {
            found.extend(profiles(&path, depth - 1)?);
        }
    }
    Ok(found)
}

fn entries(dir: &Path) -> Result<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(read) => read.map(|entry| Ok(entry?.path())).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("reading {}", dir.display())),
    }
}

fn is_directory(path: &Path) -> Result<bool> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(metadata.is_dir())
}

fn name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

/// Delete, or with `dry_run` only measure, each path; returns the files and bytes they held.
fn remove(doomed: &[PathBuf], dry_run: bool) -> Result<(u64, u64)> {
    let label = if dry_run { "Measuring" } else { "Removing" };
    let mut progress = Progress::new(label, doomed.len());
    let (mut files, mut bytes) = (0, 0);
    for (index, path) in doomed.iter().enumerate() {
        let (count, size) = size(path)?;
        if !dry_run {
            let removed = if is_directory(path)? {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            removed.with_context(|| format!("removing {}", path.display()))?;
        }
        files += count;
        bytes += size;
        progress.tick(index + 1)?;
    }
    progress.finish()?;
    Ok((files, bytes))
}

/// Files and bytes under `path`, counting a symbolic link itself and following none.
fn size(path: &Path) -> Result<(u64, u64)> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if !metadata.is_dir() {
        return Ok((1, metadata.len()));
    }
    entries(path)?
        .iter()
        .try_fold((0, 0), |(files, bytes), entry| {
            let (count, size) = size(entry)?;
            Ok((files + count, bytes + size))
        })
}

/// Bytes in binary units with one decimal, as Cargo reports what it cleaned.
fn human(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1}{}", UNITS[unit])
}

/// A Cargo-style status line: a right-aligned verb, bold green on a terminal.
fn status(verb: &str, message: &str) -> Result<()> {
    let mut stderr = std::io::stderr();
    if stderr.is_terminal() {
        writeln!(stderr, "\x1b[1;32m{verb:>12}\x1b[0m {message}")?;
    } else {
        writeln!(stderr, "{verb:>12} {message}")?;
    }
    Ok(())
}

/// A one-line bar in Cargo's style on a terminal; nothing when stderr is redirected.
struct Progress {
    label: &'static str,
    total: usize,
    drawn: Option<usize>,
    terminal: bool,
}

impl Progress {
    fn new(label: &'static str, total: usize) -> Self {
        Self {
            label,
            total,
            drawn: None,
            terminal: std::io::stderr().is_terminal(),
        }
    }

    fn tick(&mut self, done: usize) -> Result<()> {
        let filled = BAR * done / self.total.max(1);
        // Redraw only as the bar grows, so thousands of entries do not flood the terminal.
        if !self.terminal || self.drawn == Some(filled) {
            return Ok(());
        }
        self.drawn = Some(filled);
        write!(
            std::io::stderr(),
            "\r\x1b[1;36m{:>12}\x1b[0m [{}>{}] {done}/{}",
            self.label,
            "=".repeat(filled),
            " ".repeat(BAR - filled),
            self.total
        )?;
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        if self.drawn.is_some() {
            write!(std::io::stderr(), "\r\x1b[2K")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unit_hashes_come_only_from_cargo_unit_names() {
        let hash = "065fc3cbb75ad8ea";
        for name in [
            format!("libenco_host-{hash}.rlib"),
            format!("enco_host-{hash}.d"),
            format!("integration-{hash}"),
            format!("wit-bindgen-rust-{hash}.01nw01cu.0kyvu9b.rcgu.o"),
        ] {
            assert_eq!(super::unit_hash(&name), Some(hash), "{name}");
        }
        for name in [
            "enco",
            ".cargo-lock",
            "enco_host-06bjmxettrzon",
            "serde-065FC3CBB75AD8EA.rlib",
            "serde-065fc3cbb75ad8e.rlib",
        ] {
            assert_eq!(super::unit_hash(name), None, "{name}");
        }
    }
}
