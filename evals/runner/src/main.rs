mod check;
mod config;
mod enco;
mod grade;
mod home;
mod judge;
mod limits;
mod mock;
mod proxy;
mod render;
mod result;
mod run;
mod server;
mod snapshot;
mod stats;
mod task;
mod trial;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use std::{num::NonZeroUsize, path::PathBuf, process::ExitCode};
use tokio::signal::unix::{SignalKind, signal};

#[derive(Parser)]
#[command(version, about = "Evaluate Enco through its command-line interface")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run {
        #[command(flatten)]
        options: Options,
        #[arg(long)]
        variant: Option<PathBuf>,
    },
    Compare {
        #[command(flatten)]
        options: Options,
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long, value_delimiter = ',')]
        target: Vec<String>,
    },
    Regrade {
        run_dir: PathBuf,
        #[arg(long)]
        judge: bool,
    },
    Check {
        #[arg(long)]
        config: PathBuf,
        tasks: Vec<String>,
    },
}

#[derive(Args)]
pub(crate) struct Options {
    #[arg(long)]
    pub(crate) config: PathBuf,
    #[arg(long, value_delimiter = ',')]
    pub(crate) tasks: Vec<String>,
    #[arg(short, default_value_t = limits::DEFAULT_K)]
    pub(crate) k: usize,
    #[arg(long, default_value = "1")]
    pub(crate) jobs: NonZeroUsize,
}

#[tokio::main]
#[expect(
    clippy::print_stderr,
    reason = "the CLI reports failures to its caller"
)]
async fn main() -> ExitCode {
    let outcome = tokio::select! {
        outcome = execute(Cli::parse()) => outcome,
        error = interrupted() => Err(error),
    };
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

/// Resolves on SIGINT or SIGTERM; dropping the evaluation then cleans up every trial.
async fn interrupted() -> anyhow::Error {
    let signalled = async {
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    };
    match signalled.await {
        Ok(()) => anyhow!("interrupted"),
        Err(error) => anyhow::Error::new(error).context("listen for interruption"),
    }
}

async fn execute(cli: Cli) -> Result<bool> {
    let repository = std::env::current_dir()?.canonicalize()?;
    match cli.command {
        Command::Run { options, variant } => {
            run::evaluate(&repository, options, vec![("base", variant)], None).await?;
        }
        Command::Compare {
            options,
            base,
            candidate,
            target,
        } => {
            run::evaluate(
                &repository,
                options,
                vec![("base", Some(base)), ("candidate", Some(candidate))],
                Some(target),
            )
            .await?;
        }
        Command::Regrade { run_dir, judge } => run::regrade(&repository, &run_dir, judge).await?,
        Command::Check { config, tasks } => {
            return check::check(&repository, &config, tasks).await;
        }
    }
    Ok(true)
}
