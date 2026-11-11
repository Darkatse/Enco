use crate::{
    bad,
    config::{Config, Engine, Service},
    given, http, invalid,
    types::Failure,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_RESULTS: u32 = 10;
const DEFAULT_RESULTS: u32 = 5;

/// Arguments of `web_search`; `engine` is narrowed to the configured engines when the tool is
/// declared.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    /// What to search for.
    query: String,
    /// Search engine; no automatic fallback between engines.
    engine: Option<Engine>,
    /// Maximum results to return; the service may return fewer.
    #[serde(default = "default_results")]
    #[schemars(range(min = 1, max = MAX_RESULTS))]
    limit: u32,
    /// Only results after this calendar date, in YYYY-MM-DD format.
    since: Option<String>,
}

fn default_results() -> u32 {
    DEFAULT_RESULTS
}

/// A search response: the results, each in the form its engine documents.
#[derive(Deserialize)]
struct Response<R> {
    results: Vec<R>,
}

/// An Exa result as Exa documents it: only `title` and `url` are always there, and the title may
/// be empty.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExaResult {
    title: String,
    url: String,
    published_date: Option<String>,
    highlights: Option<Vec<String>>,
}

/// A TinyFish result as TinyFish documents it: `date` comes with only some results.
#[derive(Deserialize)]
struct TinyfishResult {
    title: String,
    url: String,
    snippet: String,
    date: Option<String>,
}

/// A result as `web_search` returns it; what the engine left out or left blank is null.
#[derive(Serialize)]
pub(crate) struct Found {
    title: Option<String>,
    url: String,
    published: Option<String>,
    summary: Option<String>,
}

impl From<ExaResult> for Found {
    fn from(result: ExaResult) -> Self {
        Self {
            title: given(result.title),
            url: result.url,
            published: result.published_date.and_then(given),
            summary: result
                .highlights
                .map(|highlights| highlights.join("\n"))
                .and_then(given),
        }
    }
}

impl From<TinyfishResult> for Found {
    fn from(result: TinyfishResult) -> Self {
        Self {
            title: given(result.title),
            url: result.url,
            published: result.date.and_then(given),
            summary: given(result.snippet),
        }
    }
}

pub(crate) async fn search(config: &Config, input: &str) -> Result<Value, Failure> {
    let input: Arguments = enco_sdk::parse_arguments(input)?;
    // The declaration only offers web_search when at least one engine is configured.
    let engine = input.engine.unwrap_or(config.search[0]);
    if input.query.trim().is_empty() {
        return Err(invalid("query must not be empty"));
    }
    if let Some(since) = &input.since {
        let date = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d")
            .map_err(|_| invalid("since must be a calendar date in YYYY-MM-DD format"))?;
        if date.to_string() != *since {
            return Err(invalid("since must use YYYY-MM-DD format"));
        }
    }
    let response = match engine {
        Engine::Exa => {
            let mut request = json!({"query": input.query, "numResults": input.limit, "contents": {"highlights": true}});
            if let Some(since) = &input.since {
                request["startPublishedDate"] = format!("{since}T00:00:00Z").into();
            }
            http::request(
                "POST",
                format!("{}/search", config.url(Service::Exa)),
                Some(request),
            )
            .await?
        }
        Engine::Tinyfish => {
            let mut url = url::Url::parse(config.url(Service::Tinyfish))
                .map_err(|error| bad(format!("TinyFish URL: {error}")))?;
            url.query_pairs_mut().append_pair("query", &input.query);
            if let Some(since) = &input.since {
                url.query_pairs_mut().append_pair("after_date", since);
            }
            http::request("GET", url.into(), None).await?
        }
    };
    let mut results = found(engine, &response.body)?;
    results.truncate(input.limit as usize);
    Ok(json!({"source": engine.name(), "results": results}))
}

/// The results in `engine`'s response `body`, as `web_search` returns them.
pub(crate) fn found(engine: Engine, body: &[u8]) -> Result<Vec<Found>, Failure> {
    let what = format!("{} search response", engine.name());
    match engine {
        Engine::Exa => results::<ExaResult>(&what, body),
        Engine::Tinyfish => results::<TinyfishResult>(&what, body),
    }
}

/// The results of a response whose results take the form `R`.
fn results<R>(what: &str, body: &[u8]) -> Result<Vec<Found>, Failure>
where
    R: serde::de::DeserializeOwned + Into<Found>,
{
    let response: Response<R> = http::decode(what, body)?;
    Ok(response.results.into_iter().map(Into::into).collect())
}
