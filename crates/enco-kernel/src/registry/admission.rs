use super::*;

/// Whether each registered name's active generation, with any candidates substituted, can run on
/// this node, and why not.
///
/// It depends only on the generations' own facts, never on their users.
#[derive(Default)]
pub(super) struct Admission(BTreeMap<PluginName, Result<Loaded, Vec<Cause>>>);

/// Who else exports a conflicting tool name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "owner", content = "plugins", rename_all = "snake_case")]
pub enum ToolOwner {
    /// A tool provided by the host.
    Host,
    /// Other plugins exporting the same name.
    Plugins(Vec<PluginName>),
}

pub(super) fn users(uses: &[Use], name: &PluginName) -> Vec<String> {
    uses.iter()
        .filter(|usage| usage.plugin() == name)
        .map(|usage| usage.user.clone())
        .collect()
}

pub(super) fn serve(
    usage: &Use,
    selected: Option<Result<&Loaded, &[Cause]>>,
) -> Result<(), Vec<Cause>> {
    match selected {
        Some(Ok(loaded)) => usage.check(loaded).map_err(|failure| {
            vec![Cause::Use {
                user: usage.user.clone(),
                target: usage.reference(),
                failure,
            }]
        }),
        Some(Err(_)) => Err(vec![plugin_unavailable(usage)]),
        None => Err(vec![unknown_plugin(usage)]),
    }
}

/// The plugin's causes are its own facts, not repeated for each use.
pub(super) fn plugin_unavailable(usage: &Use) -> Cause {
    Cause::PluginUnavailable {
        user: usage.user.clone(),
        target: usage.reference(),
        plugin: usage.plugin().clone(),
    }
}

pub(super) fn unknown_plugin(usage: &Use) -> Cause {
    Cause::UnknownPlugin {
        user: usage.user.clone(),
        target: usage.reference(),
        plugin: usage.plugin().clone(),
    }
}

/// Evaluate one complete proposed composition, independent of how it was assembled.
pub(super) fn evaluate(
    contents: BTreeMap<PluginName, Option<Loaded>>,
    host_tool_names: &[String],
) -> Admission {
    Admission(check(contents, host_tool_names))
}

fn check(
    contents: BTreeMap<PluginName, Option<Loaded>>,
    native: &[String],
) -> BTreeMap<PluginName, Result<Loaded, Vec<Cause>>> {
    let mut causes: BTreeMap<PluginName, Vec<Cause>> = BTreeMap::new();
    let mut tool_owners: BTreeMap<&str, Vec<&PluginName>> = BTreeMap::new();
    for (name, loaded) in &contents {
        let causes = causes.entry(name.clone()).or_default();
        let Some(loaded) = loaded else {
            causes.push(Cause::NotLoaded {
                plugin: name.clone(),
            });
            continue;
        };
        if let Some(failure) = &loaded.unavailable {
            causes.push(Cause::Environment {
                plugin: name.clone(),
                failure: failure.clone(),
            });
        }
        if let Some(tools) = &loaded.tools {
            for spec in &tools.specs {
                tool_owners.entry(&spec.name).or_default().push(name);
            }
        }
    }
    for (tool, owners) in tool_owners {
        let host = native.iter().any(|name| name == tool);
        if !host && owners.len() < 2 {
            continue;
        }
        for owner in &owners {
            let with = if host {
                ToolOwner::Host
            } else {
                ToolOwner::Plugins(
                    owners
                        .iter()
                        .filter(|other| *other != owner)
                        .map(|other| (*other).clone())
                        .collect(),
                )
            };
            causes
                .entry((*owner).clone())
                .or_default()
                .push(Cause::ToolConflict {
                    plugin: (*owner).clone(),
                    tool: tool.into(),
                    with,
                });
        }
    }
    contents
        .into_iter()
        .map(|(name, loaded)| {
            let causes = causes.remove(&name).unwrap_or_default();
            let result = match loaded {
                Some(loaded) if causes.is_empty() => Ok(loaded),
                _ => Err(causes),
            };
            (name, result)
        })
        .collect()
}

impl Admission {
    pub fn result(&self, name: &PluginName) -> Option<Result<Loaded, Vec<Cause>>> {
        self.0.get(name).cloned()
    }

    /// Absent for names not running.
    pub fn get(&self, name: &PluginName) -> Option<Result<&Loaded, &[Cause]>> {
        self.0
            .get(name)
            .map(|result| result.as_ref().map_err(Vec::as_slice))
    }

    /// Decide whether an intentional change from `before` to this admission may be accepted.
    ///
    /// An intentional change must leave its candidates and every new or changed use working,
    /// and must not stop anything that worked before. Failures that predate the change and that
    /// it does not touch stay reported without blocking it, so repairs can land one at a time.
    pub fn accept_change(
        &self,
        before: &Self,
        candidates: &[PluginName],
        uses: &[Use],
        previous: &[Use],
    ) -> Result<(), RegistryError> {
        for (name, result) in &self.0 {
            if let Err(causes) = result
                && (candidates.contains(name) || matches!(before.get(name), Some(Ok(_))))
            {
                return Err(RegistryError::Rejected(causes.clone()));
            }
        }
        for usage in uses {
            let changed = !previous
                .iter()
                .any(|old| old.user == usage.user && old.same_call(usage));
            let served = |admission: &Self| serve(usage, admission.get(usage.plugin()));
            if let Err(causes) = served(self)
                && (changed || served(before).is_ok())
            {
                return Err(RegistryError::Rejected(causes));
            }
        }
        Ok(())
    }
}
