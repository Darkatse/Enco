mod chat;
mod client;
mod compose_root;
mod daemon;
mod paths;
mod protocol;
mod send;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use enco_core::{AttemptId, EventId, GenerationId, MemoryId, ScheduleId};
use paths::Paths;
use protocol::Command;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(
    name = "enco",
    version,
    about = "A personal agent with a durable execution log"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    Config {
        #[command(subcommand)]
        command: ConfigAction,
    },
    Plugin {
        #[command(subcommand)]
        command: PluginAction,
    },
    Init,
    Serve,
    Chat {
        #[arg(long, default_value = "main")]
        session: String,
    },
    Send {
        #[arg(long, default_value = "main")]
        session: String,
        #[arg(long)]
        wait: bool,
        text: String,
    },
    Schedules {
        #[arg(long)]
        cancel: Option<ScheduleId>,
    },
    Memory {
        #[arg(long)]
        forget: Option<MemoryId>,
    },
    Status,
    Sessions,
    Log {
        #[arg(long, default_value = "main", help = "Session name or ID")]
        session: String,
    },
    Inspect {
        #[arg(long, default_value = "main", help = "Session name or ID")]
        session: String,
        #[arg(long)]
        attempt: Option<AttemptId>,
    },
    Profile {
        session: String,
        profile: String,
    },
    SafeMode {
        state: Switch,
    },
    Cancel {
        #[arg(long, default_value = "main")]
        session: String,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    Status,
    Deploy {
        name: String,
        #[arg(required_unless_present = "generation", conflicts_with = "generation")]
        path: Option<PathBuf>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        generation: Option<u64>,
    },
    Rollback {
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    Apply {
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        generation: Option<u64>,
    },
    Rollback,
}

#[derive(Clone, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[tokio::main]
#[expect(
    clippy::print_stdout,
    reason = "printing the result is this command's interface"
)]
async fn main() -> Result<()> {
    let paths = Paths::from_env()?;
    let cli = Cli::parse();
    let command = match cli.command {
        Action::Plugin { command } => match command {
            PluginAction::Status => Command::PluginStatus {},
            PluginAction::Deploy {
                name,
                path,
                generation,
            } => Command::PluginDeploy {
                name,
                source: match (path, generation) {
                    (Some(path), None) => protocol::DeploySource::Path(std::path::absolute(path)?),
                    (None, Some(id)) => protocol::DeploySource::Generation(GenerationId(id)),
                    _ => anyhow::bail!("provide exactly one of path or generation"),
                },
            },
            PluginAction::Rollback { name } => Command::PluginRollback { name },
        },
        Action::Config { command } => match command {
            ConfigAction::Apply { generation } => Command::ConfigApply {
                generation: generation.map(GenerationId),
            },
            ConfigAction::Rollback => Command::ConfigRollback {},
        },
        Action::Init => return init(&paths).await,
        Action::Serve => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_env("ENCO_LOG")
                        .unwrap_or_else(|_| "info".into()),
                )
                .with_writer(std::io::stderr)
                .init();
            return daemon::serve(paths).await;
        }
        Action::Chat { session } => return chat::chat(&paths, session).await,
        Action::Send {
            session,
            text,
            wait: true,
        } => {
            println!("{:#}", send::wait(&paths, session, text).await?);
            return Ok(());
        }
        Action::Send {
            session,
            text,
            wait: false,
        } => Command::Send {
            session,
            text,
            event_id: EventId::new(),
        },
        Action::Schedules { cancel } => match cancel {
            Some(schedule_id) => Command::CancelSchedule { schedule_id },
            None => Command::Schedules {},
        },
        Action::Memory { forget } => match forget {
            Some(memory_id) => Command::ForgetMemory { memory_id },
            None => Command::Memories {},
        },
        Action::Profile { session, profile } => Command::SetProfile { session, profile },
        Action::Status => Command::Status {},
        Action::Sessions => Command::Sessions {},
        Action::Log { session } => Command::Log {
            session,
            after: None,
        },
        Action::Inspect { session, attempt } => Command::Inspect {
            session,
            attempt_id: attempt,
        },
        Action::SafeMode { state } => Command::SafeMode {
            enabled: matches!(state, Switch::On),
        },
        Action::Cancel { session } => Command::Cancel { session },
    };
    let data = client::Client::connect(&paths.socket())
        .await?
        .request(command)
        .await?;
    println!("{data:#}");
    Ok(())
}

#[expect(
    clippy::print_stdout,
    reason = "printing the result is this command's interface"
)]
async fn init(paths: &Paths) -> Result<()> {
    tokio::fs::create_dir_all(paths.workspace()).await?;
    tokio::fs::create_dir_all(paths.data()).await?;
    create_new(&paths.gitignore(), b"/.data/\n/workspace/\n").await?;
    let template = include_str!("../../../examples/deepseek-gemini.toml");
    ensure!(
        template.matches(EXAMPLE_TIMEZONE).count() == 1,
        "the configuration template must contain `{EXAMPLE_TIMEZONE}` exactly once"
    );
    let timezone = iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<enco_core::Tz>().ok());
    let line = timezone.map_or_else(
        || format!("# {EXAMPLE_TIMEZONE}  # Set your IANA time zone before starting Enco."),
        |zone| format!("timezone = \"{zone}\""),
    );
    let config = template.replacen(EXAMPLE_TIMEZONE, &line, 1);
    if create_new(&paths.config(), config.as_bytes()).await? && timezone.is_none() {
        println!(
            "Could not detect an IANA time zone. Set timezone in {} before starting Enco.",
            paths.config().display()
        );
    }
    println!(
        "Enco home: {}\nEdit {}, set its API key environment variables, then run `enco serve` and `enco chat`.",
        paths.home.display(),
        paths.config().display()
    );
    Ok(())
}

/// The template's timezone line, replaced by the detected zone.
const EXAMPLE_TIMEZONE: &str = "timezone = \"America/New_York\"";

/// Create a file unless it exists; return whether it was created.
async fn create_new(path: &Path, bytes: &[u8]) -> Result<bool> {
    match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
    {
        Ok(mut file) => {
            file.write_all(bytes).await?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e).with_context(|| format!("create {}", path.display())),
    }
}
