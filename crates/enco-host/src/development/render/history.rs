use super::{actions, caller, code, fence, next, path, quoted, step, time};
use crate::{
    development::{
        Comparison, ComparisonMode, ContentFailure, ContentUnavailable, FileDifference,
        HISTORY_PAGE_SIZE, History, HistoryPoint,
    },
    status::{CallStep, Step},
};
use enco_core::Tz;
use enco_kernel::RegistrationDetails;

impl History {
    pub fn document(&self) -> String {
        let timezone = self.timezone;
        let Some(comparison) = &self.comparison else {
            let mut out = format!(
                "# Registration history\n\nNewest first, {HISTORY_PAGE_SIZE} per page. Times are in {timezone}.\n\n"
            );
            out.push_str(&entries(&self.registrations, timezone));
            if let Some(before) = self.before {
                out.push_str(&format!(
                    "\nMore: {}.\n",
                    step(&Step::Call(CallStep::List(Some(before))))
                ));
            }
            return out;
        };
        let mut out = if let Some(entry) = self.registrations.first() {
            let mut out = format!(
                "# Registration {}\n\n{}\n\nTimes are in {timezone}.\n\n",
                entry.id,
                provenance(entry, timezone)
            );
            out.push_str(&actions::changes(&entry.changes, true));
            out.push_str(&format!("\n## Diff from {}\n\n", point(comparison.from)));
            out
        } else {
            comparison_intro(comparison)
        };
        for unit in &comparison.units {
            if let Some(reason) = &unit.unavailable {
                out.push_str(&format!(
                    "\n### {}\n\nNot available: {}.\n",
                    path(&unit.path),
                    unavailable(reason)
                ));
            }
            for file in &unit.files {
                out.push_str(&format!("\n### {}\n\n", code(&file.path)));
                match &file.change {
                    FileDifference::Text { diff } => {
                        let fence = fence(diff, 3);
                        out.push_str(&format!("{fence}diff\n{diff}{fence}\n"));
                    }
                    FileDifference::AddedEmpty => out.push_str("Added an empty file.\n"),
                    FileDifference::RemovedEmpty => out.push_str("Removed an empty file.\n"),
                    FileDifference::Binary => out.push_str("Binary files differ.\n"),
                }
            }
        }
        out
    }
}

fn comparison_intro(comparison: &Comparison) -> String {
    let unregistered = comparison.mode == ComparisonMode::HomeChanges;
    let title = if unregistered {
        "Unregistered changes".into()
    } else {
        format!(
            "{} → {}",
            point(comparison.from),
            point(Some(comparison.to))
        )
    };
    let mut out = format!("# {title}\n\n");
    let available: Vec<_> = comparison
        .units
        .iter()
        .filter(|unit| unit.unavailable.is_none())
        .map(|unit| path(&unit.path))
        .collect();
    if comparison.units.is_empty() {
        out.push_str("The file values match.\n");
        return out;
    }
    if !available.is_empty() {
        let noun = if available.len() == 1 {
            "path differs"
        } else {
            "paths differ"
        };
        out.push_str(&format!(
            "{} {noun}: {}.\n",
            available.len(),
            available.join(", ")
        ));
    }
    let unavailable = comparison.units.len() - available.len();
    if unavailable != 0 {
        let noun = if unavailable == 1 {
            "path cannot"
        } else {
            "paths cannot"
        };
        out.push_str(&format!("{unavailable} {noun} be compared.\n"));
    }
    if !comparison.conflicting_paths.is_empty() {
        out.push_str(&format!(
            "\nThese paths name different plugins in the two registrations and cannot be rewound together: {}.\n",
            comparison.conflicting_paths.iter().map(path).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(steps) = &comparison.next {
        out.push_str(&format!("\n{}\n", next(steps)));
    }
    out
}

pub(super) fn entries(entries: &[RegistrationDetails], timezone: Tz) -> String {
    let mut out = String::new();
    for entry in entries {
        out.push_str(&format!(
            "- **registration {}** · {}\n",
            entry.id,
            provenance(entry, timezone)
        ));
        for line in actions::changes(&entry.changes, false).lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out
}

fn provenance(entry: &RegistrationDetails, timezone: Tz) -> String {
    let mut text = format!(
        "{} · {}",
        time(entry.created_at, timezone),
        caller(&entry.initiator)
    );
    if let Some(reason) = entry.initiator.reason() {
        text.push_str(&format!(" · {}", quoted(reason.as_str())));
    }
    text
}

fn point(point: Option<HistoryPoint>) -> String {
    match point {
        None => "before the first registration".into(),
        Some(HistoryPoint::Home) => "home".into(),
        Some(HistoryPoint::Registration(id)) => format!("registration {id}"),
    }
}

pub(super) fn unavailable(failure: &ContentUnavailable) -> String {
    let cause = match &failure.cause {
        ContentFailure::Missing { hash } => {
            format!("the stored content {} is missing", code(&hash.to_string()))
        }
        ContentFailure::Corrupt { reason } => format!("the stored content is corrupt: {reason}"),
        ContentFailure::Unreadable(error) => {
            format!("{} {}", super::location(&error.path), error.problem)
        }
    };
    format!("{}: {cause}", point(Some(failure.point)))
}
