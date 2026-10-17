use anyhow::{Context, Result, bail};
use cargo_metadata::{Message, Metadata};
use std::{
    io::{Read, Write},
    process::{Command, ExitStatus},
    sync::Mutex,
};

type TestOutput = (String, std::io::Result<(ExitStatus, Vec<u8>)>);

pub fn run(metadata: &[Metadata]) -> Result<()> {
    let mut groups = Vec::new();
    let mut documentation = Vec::new();
    for (manifest, workspace) in super::workspaces().iter().zip(metadata) {
        groups.push(build(manifest, workspace)?.into_iter());
        if workspace
            .packages
            .iter()
            .flat_map(|p| &p.targets)
            .any(|t| t.doctest)
        {
            let mut command = cargo(manifest);
            command.arg("--doc");
            documentation.push(command);
        }
    }
    // Interleave workspaces so the evaluator's deliberate timeout starts alongside application tests.
    let mut commands = Vec::new();
    while groups.iter().any(|group| group.len() != 0) {
        for group in &mut groups {
            if let Some((name, mut command)) = group.next() {
                command.env("ENCO_BIN", metadata[0].target_directory.join("debug/enco"));
                // With up to five harnesses, five test threads kept builds and cancellation responsive.
                if std::env::var_os("RUST_TEST_THREADS").is_none() {
                    command.env("RUST_TEST_THREADS", "5");
                }
                commands.push((name, command));
            }
        }
    }
    // Clippy and doctests do not republish host executables, so they can run alongside tests.
    let (outputs, checked) = std::thread::scope(|scope| {
        let checks = scope.spawn(|| {
            super::clippy()?;
            doctests(documentation)
        });
        let outputs = execute(commands);
        (outputs, checks.join())
    });
    let checked = checked.map_err(|_| anyhow::anyhow!("compiler checks panicked"))?;
    let mut failed = Vec::new();
    for (name, output) in outputs? {
        let (status, output) = output.with_context(|| format!("running {name}"))?;
        writeln!(std::io::stdout(), "\nTesting {name}")?;
        std::io::stdout().write_all(&output)?;
        if !status.success() {
            failed.push(name);
        }
    }
    checked?;
    if !failed.is_empty() {
        bail!("{} failed", failed.join(", "));
    }
    Ok(())
}

fn doctests(commands: Vec<Command>) -> Result<()> {
    let mut failed = false;
    for command in commands {
        let (status, output) = interleaved(command)?;
        std::io::stdout().write_all(&output)?;
        failed |= !status.success();
    }
    if failed {
        bail!("documentation tests failed");
    }
    Ok(())
}

fn cargo(manifest: &str) -> Command {
    let mut command = super::command("cargo");
    command.args(["test", "--workspace", "--manifest-path", manifest]);
    if manifest.starts_with("sdk/") || manifest.starts_with("plugins/") {
        command.args(["--target-dir", "target/plugin-build"]);
    }
    command
}

/// Cargo selects the harnesses; finish building host binaries before any test starts.
fn build(manifest: &str, metadata: &Metadata) -> Result<Vec<(String, Command)>> {
    let output = cargo(manifest)
        .args(["--no-run", "--message-format=json"])
        .output()
        .with_context(|| format!("building tests for {manifest}"))?;
    std::io::stderr().write_all(&output.stderr)?;
    let mut commands = Vec::new();
    for message in Message::parse_stream(output.stdout.as_slice()) {
        match message? {
            Message::CompilerMessage(message) => {
                if let Some(rendered) = message.message.rendered {
                    std::io::stderr().write_all(rendered.as_bytes())?;
                }
            }
            Message::CompilerArtifact(artifact) if artifact.profile.test => {
                let Some(executable) = artifact.executable else {
                    continue;
                };
                let package = metadata
                    .packages
                    .iter()
                    .find(|p| p.id == artifact.package_id)
                    .context("test package in Cargo metadata")?;
                let directory = artifact
                    .manifest_path
                    .parent()
                    .context("test package directory")?;
                let mut command = super::command(&executable);
                command
                    .current_dir(directory)
                    .env("CARGO_MANIFEST_DIR", directory)
                    .env("CARGO_MANIFEST_PATH", &artifact.manifest_path);
                let variable = if cfg!(target_os = "macos") {
                    "DYLD_FALLBACK_LIBRARY_PATH"
                } else {
                    "LD_LIBRARY_PATH"
                };
                let mut paths = vec![
                    executable
                        .parent()
                        .context("test executable directory")?
                        .as_std_path()
                        .to_owned(),
                ];
                if let Some(existing) = std::env::var_os(variable) {
                    paths.extend(std::env::split_paths(&existing));
                }
                command.env(variable, std::env::join_paths(paths)?);
                commands.push((
                    format!("{} / {}", package.name, artifact.target.name),
                    command,
                ));
            }
            _ => {}
        }
    }
    if !output.status.success() {
        bail!("building tests for {manifest} failed ({})", output.status);
    }
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(commands)
}

/// Limit both harnesses and their internal test threads; every started command is joined.
fn execute(commands: Vec<(String, Command)>) -> Result<Vec<TestOutput>> {
    let workers = std::thread::available_parallelism()?
        .get()
        .div_ceil(2)
        .min(5);
    let queue = Mutex::new(commands.into_iter());
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..workers {
            handles.push(scope.spawn(|| -> Result<Vec<TestOutput>> {
                let mut outputs = Vec::new();
                loop {
                    let next = queue
                        .lock()
                        .map_err(|_| anyhow::anyhow!("test queue poisoned"))?
                        .next();
                    let Some((name, command)) = next else { break };
                    outputs.push((name, interleaved(command)));
                }
                Ok(outputs)
            }));
        }
        let mut outputs = Vec::new();
        for handle in handles {
            outputs.extend(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("test worker panicked"))??,
            );
        }
        outputs.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(outputs)
    })
}

/// One pipe keeps diagnostics next to the test results they describe.
fn interleaved(mut command: Command) -> std::io::Result<(ExitStatus, Vec<u8>)> {
    let (mut reader, writer) = std::io::pipe()?;
    let mut child = command.stdout(writer.try_clone()?).stderr(writer).spawn()?;
    drop(command);
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok((child.wait()?, output))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_failed_test_does_not_drop_queued_tests() {
        let executable = std::env::current_exe().unwrap();
        let commands = (0..12)
            .map(|index| {
                let mut command = std::process::Command::new(&executable);
                command.arg(if index % 2 == 0 {
                    "--list"
                } else {
                    "--invalid-test-option"
                });
                (format!("case-{index:02}"), command)
            })
            .collect();
        let outputs = super::execute(commands).unwrap();
        assert_eq!(outputs.len(), 12);
        for (index, (_, output)) in outputs.into_iter().enumerate() {
            assert_eq!(output.unwrap().0.success(), index % 2 == 0);
        }
    }
}
