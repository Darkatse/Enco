//! Connection declarations, validated once when registered and consumed at connection boundaries.
use enco_core::{BaseUrl, PluginName};
use enco_kernel::{
    ChannelSettings, ConfigTable, KernelSettings, SettingsError, TableOwner, Use, UseTarget,
    parse_table,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// One configured account and its host routing policy.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSettings {
    /// Registered mapper plugin.
    pub plugin: PluginName,
    /// Service origin, with an optional base path.
    pub base_url: BaseUrl,
    /// Protocol ID of the owner.
    pub owner: String,
    /// Credential names mapped to environment variable names.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// Maximum number of earlier group messages quoted with the next input.
    #[serde(default)]
    pub context: usize,
    /// Profile for the Sessions this connection creates for groups; absent, it serves private
    /// conversations only.
    pub group_profile: Option<enco_core::ProfileName>,
    /// IDs of the realms private to the owner, such as Discord server IDs; each conversation in
    /// them is private and gets its own root Session.
    ///
    /// IDs are nonblank and matched exactly; an empty set leaves privacy to the protocol.
    #[serde(default)]
    pub private_realms: BTreeSet<String>,
    /// Whether to open a conversation for a child Session that works for its parent while the
    /// parent shows here; defaults to true.
    ///
    /// Disabling this leaves conversations already opened in use until their child is archived.
    #[serde(default = "default_show_children")]
    pub show_children: bool,
    /// The connection's name in the `channel` table; a Session it creates without a name of the
    /// conversation's own is named after it and the conversation.
    #[serde(skip)]
    pub name: String,
}

fn default_show_children() -> bool {
    true
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
        if settings
            .private_realms
            .iter()
            .any(|realm| realm.trim().is_empty())
        {
            return Err(SettingsError::new(
                &table,
                "private_realms IDs must not be empty",
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
