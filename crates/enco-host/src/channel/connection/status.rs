//! `/status` renders the kernel's overview of the conversation's Session.
//!
//! A private conversation also shows the situations `status::attention` derives from the
//! kernel's execution facts.
use super::*;
use crate::{
    clock::local_time,
    status::{AttentionKind, HostBuild},
};
use enco_kernel::{RunState, SessionOverview};

impl Connection {
    pub(super) async fn status(
        &mut self,
        mut state: State,
        key: String,
        target: &str,
        kind: &transport::ConversationKind,
    ) -> Result<Option<String>, ChannelError> {
        let (session, notice) = match self.addressed(&mut state, target, kind).await? {
            Ok(addressed) => addressed,
            Err(receipt) => return self.receipt(state, key, receipt).await,
        };
        let overview = self.kernel.session_overview(session).await?;
        let node = self.kernel.status().await?;
        let mut lines = session_lines(&overview, node.timezone);
        if kind.private() {
            lines.extend(node_lines(&node, &self.build));
        }
        let receipt = conversation::with_notice(notice, &lines.join("\n"));
        self.receipt(state, key, receipt).await
    }
}

fn session_lines(overview: &SessionOverview, timezone: Tz) -> Vec<String> {
    let session = &overview.session;
    let model = match &overview.reply {
        Some(reply) => reply.model.clone(),
        None => "this profile is not in the configuration".into(),
    };
    let mut place = format!("Working directory: {}", session.working_dir.path_str());
    if overview.children > 0 {
        let noun = if overview.children == 1 {
            "child"
        } else {
            "children"
        };
        place.push_str(&format!(
            " · {} {noun}, {} running",
            overview.children, overview.running_children
        ));
    }
    vec![
        format!("{} · profile {} · {model}", session.name, session.profile),
        run_line(&overview.run, timezone),
        context_line(overview),
        place,
    ]
}

/// The build, then one line for each situation in the kernel's execution facts.
fn node_lines(node: &enco_kernel::Status, build: &HostBuild) -> Vec<String> {
    std::iter::once(format!("Enco {} ({})", build.version, build.revision))
        .chain(
            crate::status::execution_attention(node)
                .iter()
                .map(|attention| match &attention.kind {
                    AttentionKind::SafeMode => "Safe mode is on".to_owned(),
                    AttentionKind::SchedulerStopped { reason } => {
                        format!("The Scheduler stopped: {reason}")
                    }
                    AttentionKind::SessionStopped { name, reason } => {
                        format!("Session {name} stopped: {reason}")
                    }
                    // A situation this list does not word yet still shows that one exists.
                    _ => "Something needs attention".to_owned(),
                })
                .map(|situation| format!("{situation}; run enco status for details.")),
        )
        .collect()
}

fn run_line(run: &RunState, timezone: Tz) -> String {
    let local = |at: &DateTime<Utc>| local_time(*at, timezone);
    match run {
        RunState::Idle { last_run: None } => "Idle; no Run has ended yet".into(),
        RunState::Idle {
            last_run: Some((at, end)),
        } => format!(
            "Idle since {}; last Run {}",
            local(at),
            projection::run_ended(end)
        ),
        RunState::Running { started_at, round } => {
            format!("Running since {}, Round {round}", local(started_at))
        }
        RunState::Waiting { position } => {
            format!("Waiting its turn to run (position {position})")
        }
        RunState::Halted(halt) => match &halt.by {
            Some(by) => format!("Halted by {}", by.name),
            None => "Halted by the owner".into(),
        },
        RunState::Stopped { reason } => format!("Stopped after a failure: {reason}"),
    }
}

fn context_line(overview: &SessionOverview) -> String {
    let context = &overview.context;
    let mut text = match &context.last_request {
        None => "Context: no requests in this conversation yet".into(),
        Some(last) => {
            let input = last.usage.input_tokens;
            let mut text = match &overview.reply {
                Some(reply) => format!(
                    "Context: {} of {} tokens in the last request ({}%)",
                    tokens(input),
                    tokens(u64::from(reply.window.get())),
                    input * 100 / u64::from(reply.window.get())
                ),
                None => format!("Context: {} tokens in the last request", tokens(input)),
            };
            if let Some(cached) = last.usage.cached_input_tokens.filter(|_| input > 0) {
                text.push_str(&format!(", {}% cached", cached * 100 / input));
            }
            if overview.reply.as_ref().map(|reply| reply.model.as_str()) != Some(&last.model) {
                text.push_str(&format!(" (on {})", last.model));
            }
            if last.compacted_since {
                text.push_str("; compacted since that request");
            }
            text
        }
    };
    if context.compaction_requested {
        text.push_str("; compaction requested for the next Run");
    }
    if let (Some(limit), Some(reply)) = (context.service_limit, &overview.reply) {
        text.push_str(&format!(
            "\nThe service behind `{endpoint}` turned a request away: it accepts at most {} input tokens, fewer than its configured {}-token window leaves for input. Lower `window_tokens` for `{endpoint}`.",
            tokens(limit),
            tokens(u64::from(reply.window.get())),
            endpoint = reply.endpoint,
        ));
    }
    text
}

/// A token count in thousands with one decimal, such as `61.2k`; exact below a thousand.
fn tokens(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    let tenths = (n + 50) / 100;
    if tenths.is_multiple_of(10) {
        format!("{}k", tenths / 10)
    } else {
        format!("{}.{}k", tenths / 10, tenths % 10)
    }
}
