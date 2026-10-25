//! The documents that registering, applying and rewinding return.
use super::{code, generation_state, location, next, path, quoted};
use crate::{
    development::{Applied, HistoryPoint, HistoryQuery, Rewound},
    status::{CallStep, Steps},
};
use enco_core::IntentPath;
use enco_kernel::{PluginChangeKind, Registered, UnitChange};
use std::collections::BTreeMap;

/// A registration as the Markdown document a tool result shows.
pub fn registration_document(registered: &Registered) -> String {
    let mut out = registration_facts(registered);
    out.push_str(&recheck());
    out
}

fn recheck() -> String {
    format!(
        "\n{}\n",
        next(&Steps::sequence(vec![CallStep::Recheck.into()]))
    )
}

fn registration_facts(registered: &Registered) -> String {
    let mut out = format!("# Registration {}\n\n", registered.registration.id);
    if let Some(reason) = registered.registration.initiator.reason() {
        out.push_str(&format!("{}\n\n", quoted(reason.as_str())));
    }
    out.push_str(&changes(&registered.changes, true));
    if !registered.probes.is_empty() {
        out.push_str("\nChecks:\n");
        for probe in &registered.probes {
            out.push_str(&format!(
                "- {} answered a real call through {}.\n",
                code(&probe.user),
                code(&format!(
                    "[{}.{}]",
                    enco_kernel::ENDPOINT_TABLE.name(),
                    probe.endpoint
                ))
            ));
        }
    }
    for warning in &registered.warnings {
        out.push_str(&format!("\nCleanup warning: {warning}\n"));
    }
    out
}

impl Applied {
    /// The apply as the Markdown document a tool result shows.
    pub fn document(&self) -> String {
        let mut out = self.registered.as_ref().map_or_else(
            || "# Nothing registered\n\nNo registered code or configuration would change.\n".into(),
            registration_facts,
        );
        if !self.text_only.is_empty() {
            out.push_str(&format!("\nText-only changes in {} stay in the home until those paths next change code or configuration.\n", self.text_only.iter().map(path).collect::<Vec<_>>().join(", ")));
        }
        if !self.builds.is_empty() {
            out.push_str("\nBuilds:\n");
            for build in &self.builds {
                let outcome = match (build.reused, build.code_changed) {
                    (true, true) => "reused a retained build",
                    (false, true) => "built",
                    (true, false) => "reused a retained build; the code is unchanged",
                    (false, false) => "built; the code is unchanged",
                };
                out.push_str(&format!(
                    "- {}: {outcome}.",
                    path(&IntentPath::Plugin(build.name.clone()))
                ));
                if let Some(log) = &build.log {
                    out.push_str(&format!(" Full log: {}", location(log)));
                }
                out.push('\n');
                for diagnostic in &build.diagnostics {
                    out.push_str(&format!("  - {diagnostic}\n"));
                }
            }
        }
        if let Some(registered) = &self.registered {
            let trials: Vec<_> = registered
                .changes
                .iter()
                .filter_map(|change| match change {
                    UnitChange::Plugin { change, .. } => change.generation(),
                    UnitChange::Configuration { .. } => None,
                })
                .filter(|generation| generation.status == enco_core::GenerationStatus::Trial)
                .map(|generation| generation.id.to_string())
                .collect();
            match trials.as_slice() {
                [] => {}
                [one] => out.push_str(&format!(
                    "\nNew code starts on trial: generation {one} becomes healthy once the required probe and first calls succeed.\n"
                )),
                many => out.push_str(&format!(
                    "\nNew code starts on trial: generations {} become healthy once the required probe and first calls succeed.\n",
                    many.join(", ")
                )),
            }
            out.push_str(&recheck());
        }
        out
    }
}

impl Rewound {
    /// The rewind as the Markdown document a tool result shows.
    pub fn document(&self) -> String {
        let registration = self.registration;
        if self.written.is_empty() && self.removed.is_empty() {
            let facts = if self.unchanged.is_empty() {
                format!("No path's registered version changed after registration {registration}.")
            } else {
                format!(
                    "These paths already hold registration {registration}'s version: {}.",
                    self.unchanged
                        .iter()
                        .map(path)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return format!(
                "# Rewound to registration {registration}\n\nNothing was written. {facts}\n"
            );
        }
        let mut out = format!(
            "# Rewound to registration {registration}\n\nThese paths now hold registration {registration}'s version. Nothing is registered yet.\n\n"
        );
        for (label, paths) in [
            ("Written", &self.written),
            ("Removed", &self.removed),
            ("Already matching", &self.unchanged),
        ] {
            for target in paths {
                out.push_str(&format!("- {label}: {}\n", path(target)));
            }
        }
        let steps = Steps::sequence(vec![
            CallStep::Review(HistoryQuery::Compare {
                from: None,
                to: HistoryPoint::Home,
            })
            .into(),
            CallStep::Register.into(),
        ]);
        out.push_str(&format!("\n{}\n", next(&steps)));
        out
    }
}

pub(super) fn changes(changes: &[UnitChange], failures: bool) -> String {
    let mut groups: BTreeMap<IntentPath, Vec<&UnitChange>> = BTreeMap::new();
    for change in changes {
        groups.entry(change.path()).or_default().push(change);
    }
    let mut out = String::new();
    for (target, changes) in groups {
        let created = changes.iter().any(|change| {
            matches!(
                change,
                UnitChange::Plugin {
                    change: PluginChangeKind::Created,
                    ..
                }
            )
        });
        let mut details = Vec::new();
        for change in changes {
            match change {
                UnitChange::Configuration {
                    generation,
                    sections,
                } => {
                    let tables = sections
                        .iter()
                        .map(|table| code(&format!("[{table}]")))
                        .collect::<Vec<_>>()
                        .join(", ");
                    details.push(format!("changed {tables} → generation {generation}"));
                }
                UnitChange::Plugin {
                    change: PluginChangeKind::Created,
                    ..
                } => {}
                UnitChange::Plugin { change, .. } => {
                    details.push(plugin_change(change, created, failures))
                }
            }
        }
        if details.is_empty() && created {
            details.push("created".into());
        }
        out.push_str(&format!("- {}: {}\n", path(&target), details.join("; ")));
    }
    out
}

fn plugin_change(change: &PluginChangeKind, created: bool, failures: bool) -> String {
    let mut detail = match change {
        PluginChangeKind::Created => "created".into(),
        PluginChangeKind::Renamed { from } => format!("renamed from {}", code(from)),
        PluginChangeKind::Removed => "removed".into(),
        PluginChangeKind::Code { .. } => if created {
            "created with new code"
        } else {
            "new code"
        }
        .into(),
        PluginChangeKind::Factory { .. } => if created {
            "created with factory code"
        } else {
            "factory code"
        }
        .into(),
        PluginChangeKind::Settings { .. } => "new settings".into(),
        PluginChangeKind::Artifact { .. } => if created {
            "created from a component file"
        } else {
            "component file"
        }
        .into(),
    };
    if let Some(generation) = change.generation() {
        detail.push_str(&format!(
            " → generation {} · {}",
            generation.id,
            generation_state(generation.status)
        ));
        if failures && let Some(failure) = &generation.failure {
            detail.push_str(&format!(": {}", failure.message));
        }
    }
    detail
}
