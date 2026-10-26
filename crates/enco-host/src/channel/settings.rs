//! Connection declarations, validated once when registered and consumed at connection boundaries.
use enco_core::PluginName;
use enco_kernel::{
    ChannelSettings, ConfigTable, KernelSettings, SettingsError, TableOwner, Use, UseTarget,
    parse_table,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One configured account and its host routing policy.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSettings {
    /// Registered mapper plugin.
    pub plugin: PluginName,
    /// Service origin, with an optional base path.
    pub base_url: String,
    /// Protocol ID of the owner.
    pub owner: String,
    /// Credential names mapped to environment variable names.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// Maximum number of earlier group messages quoted with the next input.
    #[serde(default)]
    pub context: usize,
    /// Profile for the Sessions this connection creates for groups; absent, it serves private
    /// chats only.
    pub group_profile: Option<String>,
    /// Configuration name used to name new group Sessions.
    #[serde(skip)]
    pub name: String,
}

impl ConnectionSettings {
    pub(super) fn transport(&self) -> ChannelSettings {
        ChannelSettings {
            base_url: self.base_url.clone(),
            secrets: self.secrets.clone(),
        }
    }
}

pub(super) fn read(
    document: &Value,
) -> Result<BTreeMap<String, ConnectionSettings>, SettingsError> {
    let mut settings: BTreeMap<String, ConnectionSettings> =
        parse_table::<Option<_>>(document, "channel")?.unwrap_or_default();
    for (name, setting) in &mut settings {
        setting.name.clone_from(name);
    }
    Ok(settings)
}

fn declare(document: &Value, kernel: &KernelSettings) -> Result<Vec<Use>, SettingsError> {
    let mut uses = Vec::new();
    let memory_tools =
        crate::memory_tool_specs().map_err(|error| SettingsError::new("channel", error))?;
    for (name, settings) in read(document)? {
        let table = format!("channel.{name}");
        let url = reqwest::Url::parse(&settings.base_url)
            .map_err(|error| SettingsError::new(&table, error))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(SettingsError::new(
                &table,
                "base_url must be an HTTP(S) URL without credentials, query or fragment",
            ));
        }
        if settings.owner.trim().is_empty()
            || settings
                .secrets
                .values()
                .any(|value| value.trim().is_empty())
        {
            return Err(SettingsError::new(
                &table,
                "owner and secret environment names must not be empty",
            ));
        }
        if let Some(name) = &settings.group_profile {
            let profile = kernel.profiles.get(name).ok_or_else(|| {
                SettingsError::new(&table, format!("unknown group_profile {name}"))
            })?;
            if profile.owner_context
                || profile
                    .tools
                    .as_ref()
                    .is_none_or(|tools| memory_tools.iter().any(|tool| tools.contains(&tool.name)))
            {
                return Err(SettingsError::new(
                    &table,
                    "group_profile must disable owner_context and use a tools whitelist without memory tools",
                ));
            }
        }
        uses.push(Use {
            user: format!("channel {name}"),
            target: UseTarget::Channel {
                name,
                plugin: settings.plugin.clone(),
                settings: settings.transport(),
            },
        });
    }
    Ok(uses)
}

/// Named connection tables, interpreted by the host and admitted by the registry.
pub const TABLE_OWNER: TableOwner = TableOwner {
    tables: &[ConfigTable::Named("channel")],
    declare,
};
