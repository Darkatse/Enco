//! The document that `self_status` returns: what needs attention, then the three layers of every
//! unit, then recent registrations, Sessions and channels.
use super::{cell, code, comparison, health, history, next, path, situations, step};
use crate::{
    channel::ChannelState,
    clock::local_time,
    development::PluginUnit,
    status::{CallStep, Status, Step},
};
use enco_core::{IntentPath, Settlement, Tz};
use enco_kernel::{Activation, CodeSource, Running};

impl Status {
    /// The status as the Markdown document `self_status` shows.
    pub fn document(&self) -> String {
        let attention = self.attention();
        let timezone = self.node.execution.timezone;
        let mut out = String::from("# Enco status\n\n");
        match attention.len() {
            0 => out.push_str("Nothing needs attention.\n"),
            1 => out.push_str("1 item needs attention.\n"),
            count => out.push_str(&format!("{count} items need attention.\n")),
        }
        if !attention.is_empty() {
            out.push_str("\n## Needs attention\n\n");
            for (index, situation) in attention.iter().enumerate() {
                out.push_str(&format!(
                    "{}. {}\n   {}\n\n",
                    index + 1,
                    situations::facts(&situation.kind),
                    next(&situation.next)
                ));
            }
        }
        out.push_str("\n## Plugins and configuration\n\n| Path | Running | Registered | Home |\n|---|---|---|---|\n");
        let config = &self.development.configuration;
        out.push_str(&format!(
            "| {} | registration {} | registration {} | {} |\n",
            path(&IntentPath::Configuration),
            config.running,
            config.registered,
            comparison(&config.home)
        ));
        for (name, plugin) in &self.development.plugins {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                path(&IntentPath::Plugin(name.clone())),
                cell(&running(plugin)),
                cell(&registered(plugin)),
                cell(&home(plugin, self.development.plugins_unreadable.is_some()))
            ));
        }
        out.push_str(&format!(
            "\n## Recent registrations\n\nTimes are in {timezone}.\n\n"
        ));
        let recent = &self.development.recent_registrations;
        out.push_str(&history::entries(recent, timezone));
        let mut more = vec![step(&Step::Call(CallStep::Show(
            self.development.registration,
        )))];
        if let Some(older) = recent.last().filter(|entry| entry.previous.is_some()) {
            more.insert(0, step(&Step::Call(CallStep::List(Some(older.id)))));
        }
        out.push_str(&format!("\nMore: {}.\n", more.join("; ")));
        node(&mut out, self, timezone);
        out
    }
}

fn running(plugin: &PluginUnit) -> String {
    let Some(status) = &plugin.running else {
        return "not running".into();
    };
    match &status.running {
        Running::Stopped => "stopped".into(),
        Running::Unavailable { activation, .. } => {
            format!("{} · cannot be served", selected(activation))
        }
        Running::Serving { activation, .. } => selected(activation),
    }
}

fn registered(plugin: &PluginUnit) -> String {
    let Some(registered) = &plugin.registered else {
        return "not registered".into();
    };
    let mut text = selected(&registered.plugin.activation);
    if registered.earlier_sdk {
        text.push_str(" · earlier SDK");
    }
    text
}

/// Selected code as a cell: the registration that last changed it, or factory code.
///
/// Its standing on this node follows; a generation that pairs settings with factory code says so.
fn selected(activation: &Activation) -> String {
    let mut text = activation.code.to_string();
    if activation.code.generation().is_some() && activation.provenance.source == CodeSource::Factory
    {
        text.push_str(" · factory code");
    }
    if let Some(standing) = &activation.health {
        text.push_str(&format!(" · {}", health(standing)));
    }
    text
}

fn home(plugin: &PluginUnit, unreadable: bool) -> String {
    let Some(home) = &plugin.home else {
        return if unreadable {
            format!("cannot read {}", code(enco_core::PLUGINS_DIRECTORY))
        } else {
            "no directory".into()
        };
    };
    let mut text = comparison(&home.code).to_owned();
    if let Some(settings) = super::settings(home.settings) {
        text.push_str(&format!("; {settings}"));
    }
    text
}

fn node(out: &mut String, status: &Status, timezone: Tz) {
    let node = &status.node;
    out.push_str(&format!(
        "\n## Node\n\nHost {} ({}) · safe mode {}\n",
        node.build.version,
        code(&node.build.revision),
        if node.execution.safe_mode {
            "on"
        } else {
            "off"
        }
    ));
    sessions(out, status);
    let channels: Vec<_> = node
        .channels
        .iter()
        .map(|channel| {
            let state = match channel.state {
                ChannelState::Opening { .. } => "opening",
                ChannelState::Running => "running",
                ChannelState::Stopped { .. } => "stopped",
                ChannelState::Duplicate { .. } => "duplicate",
            };
            format!("{} {state}", code(&channel.name))
        })
        .collect();
    if !channels.is_empty() {
        out.push_str(&format!("Channels: {}\n", channels.join(" · ")));
    }
    for channel in &node.channels {
        let name = code(&channel.name);
        match &channel.state {
            ChannelState::Opening {
                retrying: Some(failure),
            } => out.push_str(&format!(
                "- {name} retries opening after: {}\n",
                failure.reason
            )),
            ChannelState::Stopped {
                failure: Some(failure),
            } => out.push_str(&format!("- {name} stopped: {}\n", failure.reason)),
            ChannelState::Duplicate { owner } => out.push_str(&format!(
                "- {name}: its account is already owned by channel {}\n",
                code(owner)
            )),
            ChannelState::Opening { retrying: None }
            | ChannelState::Running
            | ChannelState::Stopped { failure: None } => {}
        }
        for status in &channel.recent_failures {
            let failure = &status.settlement;
            let outcome = match &failure.outcome {
                Settlement::Ok => "ok".into(),
                Settlement::Failed { failure } => format!("failed: {}", failure.message),
                Settlement::Unknown { failure } => format!("unknown: {}", failure.message),
            };
            out.push_str(&format!(
                "- {} · Session {} · target {}: {outcome}\n",
                local_time(failure.at, timezone),
                code(&status.session.name),
                code(&failure.delivery.target)
            ));
        }
    }
}

fn sessions(out: &mut String, status: &Status) {
    let mut sessions: Vec<_> = status
        .node
        .execution
        .sessions
        .iter()
        .filter(|session| session.session.archived_at.is_none())
        .collect();
    sessions.sort_by(|a, b| a.session.name.cmp(&b.session.name));
    if !sessions.is_empty() {
        out.push_str("\nSessions:\n");
        for session in sessions {
            let state = if session.stopped.is_some() {
                "stopped".into()
            } else if session.running {
                "running".into()
            } else if let Some(pos) = session.waiting {
                format!("waiting for a slot (#{pos})")
            } else {
                "idle".into()
            };
            out.push_str(&format!(
                "{}- {} · profile {} · {} · {state}\n",
                "  ".repeat(session.session.name.depth() as usize),
                code(&session.session.name),
                code(session.session.profile.as_str()),
                code(session.session.working_dir.path_str())
            ));
        }
    }
}
