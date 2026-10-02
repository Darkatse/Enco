mod chat;
mod client;
mod compose_root;
mod config;
mod daemon;
mod paths;
mod protocol;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use enco_core::{AttemptId, EventId, MemoryId, ScheduleId};
use paths::Paths;
use protocol::Command;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(name = "enco", about = "A personal agent with a durable execution log")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
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
    Deploy { name: String, path: PathBuf },
    Rollback { name: String },
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
            PluginAction::Deploy { name, path } => Command::PluginDeploy {
                name,
                path: std::path::absolute(path)?,
            },
            PluginAction::Rollback { name } => Command::PluginRollback { name },
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
        Action::Send { session, text } => Command::Send {
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
    for (path, bytes) in [
        (
            paths.config(),
            include_bytes!("../../../examples/deepseek-gemini.toml").as_slice(),
        ),
        (paths.gitignore(), b"/.data/\n/workspace/\n".as_slice()),
    ] {
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(mut file) => file.write_all(bytes).await?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| format!("create {}", path.display())),
        }
    }
    println!(
        "Enco home: {}\nEdit {}, set its API key environment variables, then run `enco serve` and `enco chat`.",
        paths.home.display(),
        paths.config().display()
    );
    Ok(())
}
