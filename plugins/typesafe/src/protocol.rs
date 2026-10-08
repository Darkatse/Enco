//! TypeSafe's named wire answers map back to the caller's question and label order.
use crate::decision::{Answer, Question, QuestionKind};
use provider_protocol::{Settings, apply_options, bad, failure, post, types::Failure};
use serde_json::{Map, Value, json};

/// The contract's allowance for service rounding (`decision.answer`).
const DECISION_SUM_TOLERANCE: f64 = 0.01;

pub async fn decide(
    settings: Settings,
    state: String,
    questions: Vec<Question>,
) -> Result<Vec<Answer>, Failure> {
    let wire_questions = questions
        .iter()
        .enumerate()
        .map(|(i, question)| encode_question(question).map(|value| (format!("q{i}"), value)))
        .collect::<Result<Map<_, _>, _>>()?;
    let mut body = json!({ "model": settings.model, "state": state, "questions": wire_questions });
    apply_options(
        &mut body,
        &settings.options,
        &["model", "state", "questions"],
    )?;
    let response = post(&settings, "systemone", body, Some(10000)).await?;
    let answers = response
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("missing decision answers"))?;
    if answers.len() != questions.len() {
        return Err(bad("decision answer count does not match question count"));
    }
    questions
        .iter()
        .enumerate()
        .map(|(i, question)| {
            let answer = answers
                .get(&format!("q{i}"))
                .ok_or_else(|| bad(format!("missing answer q{i}")))?;
            decode_answer(answer, &question.kind)
        })
        .collect()
}

fn encode_question(question: &Question) -> Result<Value, Failure> {
    let mut value = match &question.kind {
        QuestionKind::Predicate => json!({ "type": "noul" }),
        QuestionKind::Choice(labels) => {
            let mut criteria = Map::new();
            for label in labels {
                if criteria
                    .insert(label.name.clone(), json!(label.description))
                    .is_some()
                {
                    return Err(failure(
                        "provider.bad_request",
                        format!("duplicate choice label: {}", label.name),
                        false,
                    ));
                }
            }
            json!({ "type": "choice", "criteria": criteria })
        }
        QuestionKind::Score(labels) => {
            let criteria: Vec<_> = labels
                .iter()
                .map(|label| match &label.description {
                    Some(description) => format!("{}: {description}", label.name),
                    None => label.name.clone(),
                })
                .collect();
            json!({ "type": "score", "criteria": criteria })
        }
    };
    value["instructions"] = json!(question.instructions);
    Ok(value)
}

fn decode_answer(answer: &Value, kind: &QuestionKind) -> Result<Answer, Failure> {
    match (answer.get("type").and_then(Value::as_str), kind) {
        (Some("noul"), QuestionKind::Predicate) => {
            Ok(Answer::Predicate(probability(&answer["noul"])?))
        }
        (Some("choice"), QuestionKind::Choice(labels)) => {
            let probabilities = labels
                .iter()
                .map(|label| probability(&answer["probabilities"][&label.name]))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Answer::Choice(distribution(probabilities)?))
        }
        (Some("score"), QuestionKind::Score(labels)) => {
            let probabilities = (0..labels.len())
                .map(|i| probability(&answer["probabilities"][i.to_string()]))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Answer::Score(distribution(probabilities)?))
        }
        _ => Err(bad("decision answer type does not match its question")),
    }
}

fn probability(value: &Value) -> Result<f64, Failure> {
    value
        .as_f64()
        .filter(|p| (0.0..=1.0).contains(p))
        .ok_or_else(|| bad("missing or invalid decision probability"))
}

fn distribution(probabilities: Vec<f64>) -> Result<Vec<f64>, Failure> {
    if (probabilities.iter().sum::<f64>() - 1.0).abs() > DECISION_SUM_TOLERANCE {
        return Err(bad("decision probabilities do not sum to one"));
    }
    Ok(probabilities)
}
