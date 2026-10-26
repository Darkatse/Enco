mod chat;
mod client;
mod compose_root;
mod daemon;
mod paths;
mod protocol;
mod send;
mod status;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use enco_core::{AttemptId, EventId, MemoryId, Reason, RegistrationId, ScheduleId};
use paths::Paths;
use protocol::{Command, Output};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(
    name = "enco",
    version = env!("ENCO_VERSION"),
    about = "A personal agent with a durable execution log"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Build and register changed source and configuration intent in Enco's store.
    Apply {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        reason: Option<Reason>,
    },
    /// Read registration history or compare a registration number with the files in the home.
    History {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        from: Option<enco_host::development::HistoryPoint>,
        #[arg(long)]
        to: Option<enco_host::development::HistoryPoint>,
        #[arg(long)]
        before: Option<u64>,
    },
    /// Restore the files in the home; review them, then apply to register the change.
    Rewind {
        #[arg(long)]
        json: bool,
        registration: u64,
        paths: Vec<enco_core::IntentPath>,
    },
    /// Restore the still-effective parts of a registration without builds or model calls.
    Rollback,
    Plugin {
        #[command(subcommand)]
        command: PluginAction,
    },
    /// Initialize the home directories and bundled SDK without replacing existing intent.
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
        /// Attach a PNG, JPEG or WebP image; repeat for multiple images.
        #[arg(long = "image")]
        images: Vec<PathBuf>,
        #[arg(required_unless_present = "images")]
        text: Option<String>,
    },
    Schedules {
        #[arg(long)]
        cancel: Option<ScheduleId>,
    },
    Memory {
        #[arg(long)]
        forget: Option<MemoryId>,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
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
    /// Create a plugin directory for the next apply: a tool template, or a factory plugin's source.
    Scaffold { name: enco_core::PluginName },
    /// Rename registered source while preserving identity and unregistered edits.
    Rename {
        #[arg(long)]
        json: bool,
        name: enco_core::PluginName,
        to: enco_core::PluginName,
        #[arg(long)]
        reason: Option<Reason>,
    },
}

#[derive(Clone, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[tokio::main]
#[expect(clippy::too_many_lines, reason = "one dispatch arm per CLI command")]
#[expect(
    clippy::print_stdout,
    reason = "printing the result is this command's interface"
)]
async fn main() -> Result<()> {
    let paths = Paths::from_env()?;
    let cli = Cli::parse();
    let command = match cli.command {
        Action::Apply { reason, json } => Command::Apply {
            reason,
            output: Output::for_json(json),
        },
        Action::History {
            from,
            to,
            before,
            json,
        } => Command::History {
            output: Output::for_json(json),
            query: enco_host::development::HistoryQuery::new(from, to, before.map(RegistrationId))
                .map_err(|error| anyhow::anyhow!(error))?,
        },
        Action::Rewind {
            registration,
            paths,
            json,
        } => Command::Rewind {
            output: Output::for_json(json),
            arguments: enco_host::development::RewindArguments {
                registration: RegistrationId(registration),
                paths: (!paths.is_empty()).then_some(paths),
            },
        },
        Action::Rollback => Command::Rollback {},
        Action::Plugin { command } => match command {
            PluginAction::Scaffold { name } => Command::PluginScaffold { name },
            PluginAction::Rename {
                name,
                to,
                reason,
                json,
            } => Command::PluginRename {
                name,
                to,
                reason,
                output: Output::for_json(json),
            },
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
            images,
            wait: true,
        } => {
            println!(
                "{:#}",
                send::wait(&paths, session, text.unwrap_or_default(), absolute(images)?).await?
            );
            return Ok(());
        }
        Action::Send {
            session,
            text,
            images,
            wait: false,
        } => Command::Send {
            session,
            text: text.unwrap_or_default(),
            images: absolute(images)?,
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
        Action::Status { json } => Command::Status {
            output: Output::for_json(json),
        },
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
    let output = command.output();
    let data = client::Client::connect(&paths.socket())
        .await?
        .request(command)
        .await?;
    match output {
        Output::Json => println!("{data:#}"),
        Output::Document => {
            let document: String = serde_json::from_value(data)?;
            println!("{document}");
        }
    }
    Ok(())
}

#[expect(
    clippy::print_stdout,
    reason = "printing the result is this command's interface"
)]
async fn init(paths: &Paths) -> Result<()> {
    tokio::fs::create_dir_all(paths.workspace()).await?;
    tokio::fs::create_dir_all(paths.data()).await?;
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
    compose_root::sdk()?.write(&paths.home).await?;
    enco_host::development::Development::initialize(&paths.home).await?;
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
/// The daemon reads these paths from its own working directory.
fn absolute(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    Ok(paths
        .iter()
        .map(std::path::absolute)
        .collect::<Result<_, _>>()?)
}

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

impl Output {
    pub(crate) fn for_json(json: bool) -> Self {
        if json { Self::Json } else { Self::Document }
    }
}

impl Command {
    pub(crate) fn output(&self) -> Output {
        match self {
            Self::Status { output }
            | Self::Apply { output, .. }
            | Self::History { output, .. }
            | Self::Rewind { output, .. }
            | Self::PluginRename { output, .. } => *output,
            Self::Send { .. }
            | Self::Subscribe { .. }
            | Self::Cancel { .. }
            | Self::Schedules { .. }
            | Self::CancelSchedule { .. }
            | Self::Memories { .. }
            | Self::ForgetMemory { .. }
            | Self::Sessions { .. }
            | Self::Log { .. }
            | Self::Rollback { .. }
            | Self::PluginScaffold { .. }
            | Self::Inspect { .. }
            | Self::SetProfile { .. }
            | Self::SafeMode { .. } => Output::Json,
        }
    }
}
