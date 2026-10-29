//! The document that `self_status` returns: what needs attention, then the three layers of every
//! unit, then recent registrations, Sessions and channels.
use super::{
    cell, code, comparison, generation_state, history, next, path, situations, step, time,
};
use crate::{
    channel::ChannelState,
    development::PluginUnit,
    status::{CallStep, Status, Step},
};
use enco_core::{IntentPath, Settlement, Tz};
use enco_kernel::{CodeSource, Running};

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
            "| {} | generation {} | generation {} · registration {} | {} |\n",
            path(&IntentPath::Configuration),
            config.running.generation,
            config.registered.generation,
            config.registered.registration,
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
            format!("generation {} · cannot be served", activation.generation.id)
        }
        Running::Serving { activation, .. } => format!(
            "generation {} · {}",
            activation.generation.id,
            generation_state(activation.generation.status)
        ),
    }
}

fn registered(plugin: &PluginUnit) -> String {
    let Some(registered) = &plugin.registered else {
        return "not registered".into();
    };
    let activation = &registered.plugin.activation;
    let mut text = format!(
        "{}generation {} · {}",
        if activation.code.source == CodeSource::Factory {
            "factory code · "
        } else {
            ""
        },
        activation.generation.id,
        generation_state(activation.generation.status)
    );
    if let Some(registration) = activation.registration {
        text.push_str(&format!(" · registration {registration}"));
    }
    if registered.earlier_sdk {
        text.push_str(" · earlier SDK");
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
        node.version,
        code(&node.revision),
        if node.execution.safe_mode {
            "on"
        } else {
            "off"
        }
    ));
    let sessions: Vec<_> = node
        .execution
        .sessions
        .iter()
        .map(|session| {
            format!(
                "{} {}",
                code(&session.session.name),
                if session.stopped.is_some() {
                    "stopped"
                } else if session.running {
                    "running"
                } else {
                    "idle"
                }
            )
        })
        .collect();
    if !sessions.is_empty() {
        out.push_str(&format!("Sessions: {}\n", sessions.join(" · ")));
    }
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
                time(failure.at, timezone),
                code(&status.session.name),
                code(&failure.delivery.target)
            ));
        }
    }
}
