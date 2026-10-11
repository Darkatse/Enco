use crate::{
    limits::ALPHA,
    result::{Report, Trial, TrialOutcome},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(crate) struct Comparison {
    pub(crate) targets: Vec<String>,
    pub(crate) verdict: Verdict,
    pub(crate) reasons: Vec<String>,
    pub(crate) p_better_targets: Option<f64>,
    pub(crate) tasks: BTreeMap<String, ComparedTask>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ComparedTask {
    pub(crate) role: Role,
    pub(crate) base: [usize; 2],
    pub(crate) candidate: [usize; 2],
    pub(crate) p_better: Option<f64>,
    pub(crate) p_worse: Option<f64>,
    pub(crate) usage: ComparedUsage,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    Accept,
    Reject,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Target,
    Guard,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ComparedUsage {
    base: Medians,
    candidate: Medians,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Medians {
    input_tokens: Option<f64>,
    output_tokens: Option<f64>,
    cached_input_tokens: Option<f64>,
    rounds: Option<f64>,
    wall_secs: Option<f64>,
}

pub(crate) fn compare(report: &Report, targets: &[String]) -> Result<Comparison> {
    let mut tasks = BTreeMap::new();
    let mut reasons = Vec::new();
    let mut combined = vec![1.0];
    let mut observed = 0;
    let mut valid = true;
    for task in report.tasks.keys() {
        let base = report
            .summary
            .get("base")
            .and_then(|s| s.get(task))
            .context("missing base results")?;
        let candidate = report
            .summary
            .get("candidate")
            .and_then(|s| s.get(task))
            .context("missing candidate results")?;
        let target = targets.contains(task);
        let (p_better, p_worse) = if base.valid == 0 || candidate.valid == 0 {
            reasons.push(format!("{task}: no valid samples; cannot compare"));
            valid = false;
            (None, None)
        } else {
            let distribution =
                hypergeom(base.valid, candidate.valid, base.passes + candidate.passes);
            let better = tail(&distribution, candidate.passes);
            let worse = distribution[..=candidate.passes]
                .iter()
                .sum::<f64>()
                .min(1.0);
            if target {
                combined = convolve(&combined, &distribution);
                observed += candidate.passes;
            } else if worse < ALPHA {
                reasons.push(format!("{task}: regression (p={worse:.6})"));
            }
            (Some(better), Some(worse))
        };
        tasks.insert(
            task.clone(),
            ComparedTask {
                role: if target { Role::Target } else { Role::Guard },
                base: [base.passes, base.valid],
                candidate: [candidate.passes, candidate.valid],
                p_better,
                p_worse,
                usage: ComparedUsage {
                    base: medians(report, task, "base"),
                    candidate: medians(report, task, "candidate"),
                },
            },
        );
    }
    let p_better_targets = if !targets.is_empty() && valid {
        let p = tail(&combined, observed);
        if p >= ALPHA {
            reasons.push(format!("targets: improvement not established (p={p:.6})"));
        }
        Some(p)
    } else {
        None
    };
    Ok(Comparison {
        targets: targets.to_vec(),
        verdict: if reasons.is_empty() {
            Verdict::Accept
        } else {
            Verdict::Reject
        },
        reasons,
        p_better_targets,
        tasks,
    })
}

fn hypergeom(base: usize, candidate: usize, successes: usize) -> Vec<f64> {
    let n = base + candidate;
    let mut factorials = vec![0.0; n + 1];
    for i in 1..=n {
        factorials[i] = factorials[i - 1] + (i as f64).ln();
    }
    let choose = |n: usize, k: usize| factorials[n] - factorials[k] - factorials[n - k];
    let mut probabilities = vec![0.0; candidate + 1];
    let low = candidate.saturating_sub(n - successes);
    for (b, probability) in probabilities
        .iter_mut()
        .enumerate()
        .take(candidate.min(successes) + 1)
        .skip(low)
    {
        *probability = (choose(successes, b) + choose(n - successes, candidate - b)
            - choose(n, candidate))
        .exp();
    }
    let sum: f64 = probabilities.iter().sum();
    for p in &mut probabilities {
        *p /= sum;
    }
    probabilities
}

fn convolve(left: &[f64], right: &[f64]) -> Vec<f64> {
    let mut result = vec![0.0; left.len() + right.len() - 1];
    for (i, a) in left.iter().enumerate() {
        for (j, b) in right.iter().enumerate() {
            result[i + j] += a * b;
        }
    }
    result
}

fn tail(distribution: &[f64], observed: usize) -> f64 {
    distribution[observed..].iter().sum::<f64>().min(1.0)
}

fn medians(report: &Report, task: &str, variant: &str) -> Medians {
    let trials: Vec<_> = report
        .trials
        .iter()
        .filter(|trial| {
            trial.task == task
                && trial.variant == variant
                && trial.result != TrialOutcome::InstrumentError
        })
        .collect();
    let median = |value: fn(&Trial) -> Option<f64>| {
        let values: Option<Vec<_>> = trials.iter().map(|trial| value(trial)).collect();
        let mut values = values?;
        values.sort_by(f64::total_cmp);
        let middle = values.len() / 2;
        let high = *values.get(middle)?;
        Some(if values.len() % 2 == 0 {
            (values[middle - 1] + high) / 2.0
        } else {
            high
        })
    };
    Medians {
        input_tokens: median(|t| Some(t.usage.input_tokens as f64)),
        output_tokens: median(|t| Some(t.usage.output_tokens as f64)),
        cached_input_tokens: median(|t| t.usage.cached_input_tokens.map(|n| n as f64)),
        rounds: median(|t| Some(t.rounds as f64)),
        wall_secs: median(|t| Some(t.wall_secs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_tails_match_sensitivity_and_stratified_probability() {
        for (n, a, b, expected) in [(5, 5, 2, 1.0 / 12.0), (5, 5, 1, 1.0 / 42.0)] {
            let p: f64 = hypergeom(n, n, a + b)[..=b].iter().sum();
            assert!((p - expected).abs() < 1e-12, "{n}: {a} -> {b}: {p}");
        }
        // Two strata with two trials each: all four successes land on the candidate with probability 1/36.
        let distribution = hypergeom(2, 2, 2);
        assert!((tail(&convolve(&distribution, &distribution), 4) - 1.0 / 36.0).abs() < 1e-12);
        assert_eq!(hypergeom(5, 5, 0)[0], 1.0);
    }
}
