use crate::{
    bad,
    config::{Config, Engine, Service},
    given, http, invalid,
    types::Failure,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Why Jina Reader or Exa returned no page: it has no text they can read.
const NO_TEXT: &str = "the page has no readable text";

/// Arguments of `web_fetch`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    /// HTTP or HTTPS URL of the page to read.
    url: String,
}

/// Jina Reader's JSON answer as it is observed: Jina publishes no schema for it.
#[derive(Deserialize)]
struct JinaResponse {
    data: JinaPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JinaPage {
    content: String,
    /// The status the page itself answered with.
    http_status: u16,
    /// Absent or empty when Jina has nothing to warn about.
    warning: Option<String>,
}

/// Exa's answer to a contents request as Exa documents it, where every field is optional.
#[derive(Deserialize)]
struct ExaContents {
    #[serde(default)]
    results: Vec<ExaPage>,
    /// How retrieving each requested URL went.
    #[serde(default)]
    statuses: Vec<ExaStatus>,
}

#[derive(Deserialize)]
struct ExaPage {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ExaStatus {
    Success {},
    Error { error: Option<ExaError> },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExaError {
    tag: Option<String>,
    http_status_code: Option<u16>,
}

pub(crate) async fn fetch(config: &Config, input: &str) -> Result<String, Failure> {
    let input: Arguments = enco_sdk::parse_arguments(input)?;
    let url = url::Url::parse(&input.url).map_err(|error| invalid(format!("url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid("url must use http or https"));
    }
    let jina_url = format!("{}/{}", config.url(Service::Jina), input.url);
    // An answer that breaks Jina's contract fails here rather than being covered by Exa.
    let reason = match http::request("GET", jina_url, None).await {
        Err(failure) => failure,
        Ok(response) => match jina(&input.url, &response.body)? {
            Reading::Page(page) => return Ok(page),
            Reading::Unavailable(reason) => reason,
        },
    };
    if !config.search.contains(&Engine::Exa) {
        return Err(reason);
    }
    http::request(
        "POST",
        format!("{}/contents", config.url(Service::Exa)),
        Some(json!({"urls": [input.url], "text": true})),
    )
    .await
    .and_then(|response| exa(&input.url, &reason.message, &response.body))
    .map_err(|second| {
        bad(format!(
            "Jina Reader: {}; Exa: {}",
            reason.message, second.message
        ))
    })
}

/// What Jina Reader's answer says about a page.
pub(crate) enum Reading {
    /// The page as the tool returns it.
    Page(String),
    /// Why Jina could not read the page, which Exa may still read.
    Unavailable(Failure),
}

/// What Jina Reader answered in `body` for `url`; an answer that breaks Jina's contract fails.
pub(crate) fn jina(url: &str, body: &[u8]) -> Result<Reading, Failure> {
    let JinaResponse { data } = http::decode("Jina Reader response", body)?;
    if data.http_status >= 400 {
        let reason = bad(format!("page returned HTTP {}", data.http_status));
        return Ok(Reading::Unavailable(reason));
    }
    let Some(content) = given(data.content) else {
        return Ok(Reading::Unavailable(bad(NO_TEXT)));
    };
    let mut page = format!("Source: Jina Reader\nURL: {url}\n\n{content}");
    if let Some(warning) = data.warning.and_then(given) {
        page.push_str(&format!("\n\nWarning: {warning}"));
    }
    Ok(Reading::Page(page))
}

/// The page Exa answered with in `body` for the one URL requested, or why it gave none;
/// `reason` is why Jina Reader gave none.
pub(crate) fn exa(url: &str, reason: &str, body: &[u8]) -> Result<String, Failure> {
    let contents: ExaContents = http::decode("Exa contents response", body)?;
    if let Some(ExaStatus::Error { error }) = contents.statuses.first() {
        return Err(bad(format!(
            "could not retrieve the page: {}",
            describe(error.as_ref())
        )));
    }
    let page = contents
        .results
        .into_iter()
        .next()
        .ok_or_else(|| bad("Exa returned no page"))?;
    let text = page.text.and_then(given).ok_or_else(|| bad(NO_TEXT))?;
    Ok(format!(
        "Source: Exa\nJina Reader failed: {reason}\nURL: {url}\n\n{text}"
    ))
}

/// What Exa said about a failed retrieval, which may be nothing.
fn describe(error: Option<&ExaError>) -> String {
    let (tag, status) = error.map_or((None, None), |error| {
        (error.tag.as_deref(), error.http_status_code)
    });
    match (tag, status) {
        (Some(tag), Some(status)) => format!("{tag} (HTTP {status})"),
        (Some(tag), None) => tag.into(),
        (None, Some(status)) => format!("HTTP {status}"),
        (None, None) => "Exa gave no reason".into(),
    }
}
