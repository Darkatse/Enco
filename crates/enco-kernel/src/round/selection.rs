use super::run_entries;
use crate::{run::RoundError, session::SessionActor, *};
use enco_core::*;
use std::sync::Arc;

/// The endpoint a purpose uses now, and the code currently behind it.
pub(super) struct Choice<'p> {
    pub endpoint: &'p CompletionEndpoint,
    pub export: Export<dyn Provider>,
}

/// One purpose's first usable candidate, or why none is usable, with the candidates passed over.
struct Pick<'p> {
    chosen: Result<Choice<'p>, Failure>,
    skipped: Vec<EndpointFailure>,
}

/// Both purposes picked against one view, so the composer may need either.
pub(super) struct Selection<'p> {
    view: Arc<View>,
    reply: Pick<'p>,
    compaction: Pick<'p>,
}

impl<'p> Selection<'p> {
    /// What the composer needs about the reply model; the Round fails when none is usable.
    pub fn reply_target(&self) -> Result<ReplyTarget, RoundError> {
        let choice = self
            .reply
            .chosen
            .as_ref()
            .map_err(|failure| RoundError::Failed(failure.clone()))?;
        Ok(ReplyTarget {
            budget: choice.endpoint.budget,
            skipped: self.reply.skipped.clone(),
        })
    }

    pub fn compaction_budget(&self) -> Option<Budget> {
        let choice = self.compaction.chosen.as_ref().ok()?;
        Some(choice.endpoint.budget)
    }

    pub fn choose(&self, purpose: AttemptPurpose) -> Result<&Choice<'p>, RoundError> {
        let pick = match purpose {
            AttemptPurpose::Reply => &self.reply,
            AttemptPurpose::Compaction => &self.compaction,
        };
        pick.chosen
            .as_ref()
            .map_err(|failure| RoundError::Failed(failure.clone()))
    }

    pub fn exports(&self) -> &Exports {
        &self.view.exports
    }
}

impl SessionActor {
    /// Pick both purposes against the latest view and this Run's Log.
    pub(super) fn select<'p>(&self, profile: &'p Profile, safe_mode: bool) -> Selection<'p> {
        let view = self.deps.registry.view();
        let entries = run_entries(&self.entries);
        let pick = |endpoints, purpose| pick(endpoints, purpose, &view.exports, entries, safe_mode);
        Selection {
            reply: pick(&profile.reply, AttemptPurpose::Reply),
            compaction: pick(&profile.compaction, AttemptPurpose::Compaction),
            view,
        }
    }
}

/// The first candidate in configured order whose code resolves and whose combination is not
/// exhausted. When none is usable, the latest recorded failure explains why, or else the last
/// unavailability.
fn pick<'p>(
    endpoints: &'p Endpoints,
    purpose: AttemptPurpose,
    exports: &Exports,
    entries: &[Entry],
    safe_mode: bool,
) -> Pick<'p> {
    let assess = |endpoint| assess(endpoint, purpose, exports, entries, safe_mode);
    let passed_over = |endpoint: &CompletionEndpoint, failure: &Failure| EndpointFailure {
        endpoint: endpoint.endpoint.name.clone(),
        failure: failure.clone(),
    };
    let mut latest = match assess(endpoints.first()) {
        Ok(choice) => {
            return Pick {
                chosen: Ok(choice),
                skipped: Vec::new(),
            };
        }
        Err(failure) => failure,
    };
    let mut skipped = vec![passed_over(endpoints.first(), &latest.1)];
    for endpoint in endpoints.rest() {
        match assess(endpoint) {
            Ok(choice) => {
                return Pick {
                    chosen: Ok(choice),
                    skipped,
                };
            }
            Err(failure) => {
                skipped.push(passed_over(endpoint, &failure.1));
                if failure.0 >= latest.0 {
                    latest = failure;
                }
            }
        }
    }
    Pick {
        chosen: Err(latest.1),
        skipped,
    }
}

/// A candidate's current code, unless its combination is exhausted in this Run. A failure
/// carries the position of its record; a current unavailability has none.
fn assess<'p>(
    endpoint: &'p CompletionEndpoint,
    purpose: AttemptPurpose,
    exports: &Exports,
    entries: &[Entry],
    safe_mode: bool,
) -> Result<Choice<'p>, (Option<LogPos>, Failure)> {
    let export = exports
        .completion(&endpoint.endpoint.plugin, safe_mode)
        .map_err(|failure| (None, failure))?;
    let combination = Combination {
        purpose,
        settings: &endpoint.endpoint.settings,
        provider: export.generation,
    };
    match combination.exhausted(entries) {
        Some((pos, failure)) => Err((Some(pos), failure)),
        None => Ok(Choice { endpoint, export }),
    }
}

/// A purpose, call parameters and exact Provider generation share one attempt allowance.
struct Combination<'a> {
    purpose: AttemptPurpose,
    settings: &'a ProviderSettings,
    provider: GenerationId,
}

impl Combination<'_> {
    /// Exhausted when, in any Round of the Run, its last Attempt failed or was unusable.
    fn exhausted(&self, run: &[Entry]) -> Option<(LogPos, Failure)> {
        let mut started = std::collections::HashMap::new();
        let mut rounds = std::collections::HashMap::new();
        for entry in run {
            match &entry.body {
                EntryBody::AttemptStarted {
                    attempt,
                    round,
                    purpose,
                    settings,
                    provider,
                    ..
                } if *purpose == self.purpose
                    && settings == self.settings
                    && *provider == self.provider =>
                {
                    started.insert(*attempt, *round);
                }
                EntryBody::AttemptSettled { attempt, result } => {
                    if let Some(round) = started.remove(attempt) {
                        rounds.insert(round, (entry.pos, result.failure(self.purpose)));
                    }
                }
                _ => {}
            }
        }
        rounds
            .into_values()
            .filter_map(|(pos, failure)| Some((pos, failure?)))
            .max_by_key(|(pos, _)| *pos)
    }
}
