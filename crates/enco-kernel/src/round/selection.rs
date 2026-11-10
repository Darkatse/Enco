use super::{context::Measured, run_entries, this_round};
use crate::{run::RoundError, session::SessionActor, *};
use enco_core::*;
use std::collections::HashMap;

/// The endpoint a purpose uses now, and the code currently behind it.
pub(super) struct Choice<'p> {
    pub endpoint: &'p CompletionEndpoint,
    pub export: Export<dyn Provider>,
}

impl Choice<'_> {
    /// The combination this choice sends its requests to.
    pub fn combination(&self) -> Combination {
        Combination {
            settings: self.endpoint.endpoint.settings.clone(),
            provider: self.export.code,
        }
    }
}

/// One purpose's first usable candidate, or why none is usable, with the candidates passed over.
struct Pick<'p> {
    chosen: Result<Choice<'p>, Failure>,
    skipped: Vec<EndpointFailure>,
}

/// Both purposes picked against one view, so the composer may need either.
pub(super) struct Selection<'p> {
    reply: Pick<'p>,
    compaction: Pick<'p>,
}

impl<'p> Selection<'p> {
    pub fn choose(&self, purpose: AttemptPurpose) -> Result<&Choice<'p>, RoundError> {
        let pick = match purpose {
            AttemptPurpose::Reply => &self.reply,
            AttemptPurpose::Compaction => &self.compaction,
        };
        pick.chosen
            .as_ref()
            .map_err(|failure| RoundError::Failed(failure.clone()))
    }
}

impl SessionActor {
    /// Pick both purposes against the latest view and this Run's Log.
    pub(super) fn select<'p>(&self, profile: &'p Profile, safe_mode: bool) -> Selection<'p> {
        let view = self.deps.registry.view();
        let entries = run_entries(self.tail.entries());
        let pick = |endpoints, purpose| pick(endpoints, purpose, &view.exports, entries, safe_mode);
        Selection {
            reply: pick(&profile.reply, AttemptPurpose::Reply),
            compaction: pick(&profile.compaction, AttemptPurpose::Compaction),
        }
    }

    /// Each purpose's target, built from the facts of the combination serving it; the Round fails
    /// when no reply candidate is usable.
    ///
    /// The reply target carries the measurement only when its combination took it, and every
    /// target carries the latest plan its combination turned away in this Round that `transcript`
    /// still holds, whichever purpose that plan was for.
    pub(super) async fn targets(
        &self,
        selection: &Selection<'_>,
        measured: Option<&Measured>,
        transcript: &Transcript,
    ) -> Result<(ReplyTarget, Option<CompactionTarget>), RoundError> {
        let choice = selection.choose(AttemptPurpose::Reply)?;
        let combination = choice.combination();
        let reply = ReplyTarget {
            budget: choice.endpoint.budget,
            accepts: choice.endpoint.accepts.clone(),
            skipped: selection.reply.skipped.clone(),
            previous_usage: measured
                .filter(|measured| measured.combination == combination)
                .map(|measured| measured.usage.clone()),
            overflowed: self.overflowed(&combination, transcript).await?,
        };
        let compaction = match selection.choose(AttemptPurpose::Compaction) {
            Ok(choice) => Some(CompactionTarget {
                budget: choice.endpoint.budget,
                overflowed: self.overflowed(&choice.combination(), transcript).await?,
            }),
            Err(_) => None,
        };
        Ok((reply, compaction))
    }

    /// The latest plan `combination` turned away in this Round that `transcript` still holds; a
    /// new Round composes for the configured window again.
    ///
    /// A plan whose Log items a compaction has since summarized can no longer be measured against
    /// the conversation, so an earlier refusal stands in for it. The plans are read back from the
    /// store; one that cannot be read stops the Session, as any store failure does.
    async fn overflowed(
        &self,
        combination: &Combination,
        transcript: &Transcript,
    ) -> Result<Option<Overflowed>, RoundError> {
        for (plan, overflow) in combination.overflowed(this_round(self.tail.entries())) {
            let plan = self.deps.store.plan(plan).await?;
            if crate::plan::held(&plan, transcript) {
                return Ok(Some(Overflowed {
                    plan,
                    overflow: overflow.clone(),
                }));
            }
        }
        Ok(None)
    }
}

/// The first candidate in configured order whose code resolves and whose combination is not
/// exhausted.
///
/// When none is usable, the latest recorded failure explains why, or else the last
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
        .completion(&endpoint.endpoint, safe_mode)
        .map_err(|failure| (None, failure))?;
    let choice = Choice { endpoint, export };
    match choice.combination().exhausted(purpose, entries) {
        Some((pos, failure)) => Err((Some(pos), failure)),
        None => Ok(choice),
    }
}

/// The call parameters and exact Provider code a model request goes to.
///
/// Requests to one combination count tokens the same way and meet the same service limits, and
/// each purpose has one attempt allowance on it.
#[derive(Clone, PartialEq)]
pub(super) struct Combination {
    pub settings: ProviderSettings,
    pub provider: CodeRef,
}

impl Combination {
    /// Whether `body` starts an Attempt on this combination.
    fn took(&self, body: &EntryBody) -> bool {
        matches!(
            body,
            EntryBody::AttemptStarted { settings, provider, .. }
                if *settings == self.settings && *provider == self.provider
        )
    }

    /// Exhausted for `purpose` when, in any Round of the Run, its last Attempt for that purpose
    /// failed or was unusable.
    fn exhausted(&self, purpose: AttemptPurpose, run: &[Entry]) -> Option<(LogPos, Failure)> {
        let mut started = HashMap::new();
        let mut rounds = HashMap::new();
        for entry in run {
            match &entry.body {
                EntryBody::AttemptStarted {
                    attempt,
                    round,
                    purpose: started_for,
                    ..
                } if *started_for == purpose && self.took(&entry.body) => {
                    started.insert(*attempt, *round);
                }
                EntryBody::AttemptSettled {
                    attempt, result, ..
                } => {
                    if let Some(round) = started.remove(attempt) {
                        rounds.insert(round, (entry.pos, result.failure(purpose)));
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

    /// The plans of the Attempts in `entries` that this combination turned away as too large,
    /// for either purpose, latest first, with what the service said.
    fn overflowed<'e>(&self, entries: &'e [Entry]) -> Vec<(&'e ContentHash, &'e Overflow)> {
        let mut plans = HashMap::new();
        let mut refused = Vec::new();
        for entry in entries {
            match &entry.body {
                EntryBody::AttemptStarted { attempt, plan, .. } if self.took(&entry.body) => {
                    plans.insert(*attempt, plan);
                }
                EntryBody::AttemptSettled {
                    attempt,
                    result: AttemptResult::Overflow { overflow },
                    ..
                } => {
                    if let Some(plan) = plans.get(attempt) {
                        refused.push((*plan, overflow));
                    }
                }
                _ => {}
            }
        }
        refused.reverse();
        refused
    }
}

#[cfg(test)]
mod tests;
