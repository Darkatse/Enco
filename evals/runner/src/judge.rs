use crate::{
    config::Judge,
    grade::{Grade, Kind},
    limits::{GRADER_TIMEOUT, JUDGE_RETRIES},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const PROMPT: &str = "You are grading one criterion of an evaluation of a personal assistant. You see the criterion, a reference written by the task author, and the evidence: conversation transcripts and files from the assistant's environment after the run.\n\nJudge only this criterion, against the reference. The reference states what passes and what fails; follow it even where you would judge differently, and add no requirements of your own. Paraphrase, translation, formatting and length do not matter unless the reference says so.\n\nFirst explain your reasoning briefly, citing the evidence. Then end with a line containing exactly one word: PASS if the evidence meets the criterion, FAIL if it does not, or UNKNOWN if the evidence is insufficient or the reference does not decide the case.";

#[derive(Clone)]
pub(crate) struct Client {
    config: Judge,
    http: reqwest::Client,
    key: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Record {
    request: Value,
    attempts: Vec<Attempt>,
}

#[derive(Serialize, Deserialize)]
struct Attempt {
    status: Option<u16>,
    response: Option<String>,
    error: Option<String>,
}

impl Client {
    pub(crate) fn new(config: Judge) -> Result<Self> {
        let key = std::env::var(&config.api_key_env)
            .with_context(|| format!("judge key variable {} is not set", config.api_key_env))?;
        let http = reqwest::Client::builder().timeout(GRADER_TIMEOUT).build()?;
        Ok(Self { config, http, key })
    }

    pub(crate) async fn grade(&self, evidence: String, path: &Path) -> Result<Grade> {
        let request = json!({
            "model": self.config.model, "temperature": self.config.temperature,
            "messages": [{ "role": "system", "content": PROMPT }, { "role": "user", "content": evidence }],
        });
        let mut record = Record {
            request,
            attempts: Vec::new(),
        };
        for _ in 0..=JUDGE_RETRIES {
            let attempt = self.request(&record.request).await;
            let complete = attempt.error.is_none();
            record.attempts.push(attempt);
            if complete {
                break;
            }
        }
        crate::result::write_json(path, &record).await?;
        Ok(record.grade())
    }

    async fn request(&self, request: &Value) -> Attempt {
        let response = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.key)
            .json(request)
            .send()
            .await;
        match response {
            Ok(response) => {
                let status = response.status();
                match response.text().await {
                    Ok(body) => Attempt {
                        status: Some(status.as_u16()),
                        response: Some(body),
                        error: (!status.is_success()).then(|| format!("judge HTTP {status}")),
                    },
                    Err(error) => Attempt {
                        status: Some(status.as_u16()),
                        response: None,
                        error: Some(cause(error)),
                    },
                }
            }
            Err(error) => Attempt {
                status: None,
                response: None,
                error: Some(cause(error)),
            },
        }
    }
}

impl Record {
    pub(crate) fn grade(&self) -> Grade {
        let result = || -> Result<Grade> {
            let attempt = self
                .attempts
                .last()
                .context("judge record has no attempts")?;
            if let Some(error) = &attempt.error {
                return Ok(Grade::new(Kind::Error, error));
            }
            let response: Value = serde_json::from_str(
                attempt
                    .response
                    .as_deref()
                    .context("judge returned no body")?,
            )?;
            let content = response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .context("judge returned no text")?;
            let text = content.trim_end();
            let (reason, verdict) = text.rsplit_once('\n').unwrap_or(("", text));
            Ok(match verdict.trim() {
                "PASS" => Grade::new(Kind::Pass, reason.trim()),
                "FAIL" => Grade::new(Kind::Fail, reason.trim()),
                "UNKNOWN" => Grade::new(
                    Kind::Fail,
                    format!(
                        "UNKNOWN counts as failure: the assistant did not provide enough evidence; ambiguous references belong in task calibration. {}",
                        reason.trim()
                    ),
                ),
                _ => Grade::new(
                    Kind::Error,
                    format!("judge verdict is not PASS, FAIL or UNKNOWN: {text}"),
                ),
            })
        };
        result()
            .unwrap_or_else(|error| Grade::new(Kind::Error, format!("judge response: {error:#}")))
    }
}

/// reqwest's own message omits the causes.
fn cause(error: reqwest::Error) -> String {
    format!("{:#}", anyhow::Error::new(error.without_url()))
}

pub(crate) fn prompt_hash() -> String {
    blake3::hash(PROMPT.as_bytes()).to_hex().to_string()
}
