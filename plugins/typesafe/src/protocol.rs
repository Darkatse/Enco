//! TypeSafe's wire format: questions go out keyed by position, and the named answers that come back
//! map to the caller's question and label order.
use crate::decision::{Answer, Question, QuestionKind};
use enco_sdk::protocol::{
    HttpRequest, HttpResponse, Settings, apply_options, bad, bad_request, json, post_json,
    types::Failure,
};
use serde_json::{Map, Value, json};

/// The contract's allowance for service rounding (`decision.answer`).
const DECISION_SUM_TOLERANCE: f64 = 0.01;

/// How long a decision request may take, in milliseconds.
const TIMEOUT_MS: u32 = 10_000;

/// The request that asks `/systemone` to judge `questions` against `state`.
pub fn encode(
    settings: &Settings,
    state: &str,
    questions: &[Question],
) -> Result<HttpRequest, Failure> {
    let wire_questions = questions
        .iter()
        .enumerate()
        .map(|(i, question)| encode_question(question).map(|value| (key(i), value)))
        .collect::<Result<Map<_, _>, _>>()?;
    let mut body = json!({ "model": settings.model, "state": state, "questions": wire_questions });
    apply_options(
        &mut body,
        &settings.options,
        &["model", "state", "questions"],
    )?;
    Ok(post_json("/systemone", &body, Some(TIMEOUT_MS)))
}

/// One answer per question, in order, from the service's response to [`encode`]'s request.
pub fn decode(questions: &[Question], response: &HttpResponse) -> Result<Vec<Answer>, Failure> {
    let response = json(response)?;
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
                .get(&key(i))
                .ok_or_else(|| bad(format!("missing answer {}", key(i))))?;
            decode_answer(answer, &question.kind)
        })
        .collect()
}

/// The wire key of the question at `index`.
fn key(index: usize) -> String {
    format!("q{index}")
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
                    return Err(bad_request(format!(
                        "duplicate choice label: {}",
                        label.name
                    )));
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
