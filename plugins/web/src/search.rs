use crate::{
    bad,
    config::{Config, Engine, Service},
    http, invalid,
    types::Failure,
};
use schemars::JsonSchema;
use serde::Deserialize;
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
            http::json(
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
            http::json("GET", url.into(), None).await?
        }
    };
    let results = response["results"]
        .as_array()
        .ok_or_else(|| bad("search response has no results array"))?;
    let results = results
        .iter()
        .take(input.limit as usize)
        .map(|result| normalize(engine, result))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"source": engine.name(), "results": results}))
}

fn normalize(engine: Engine, result: &Value) -> Result<Value, Failure> {
    let title = http::text(result, "title")?;
    let url = http::text(result, "url")?;
    let (date, summary) = match engine {
        Engine::Exa => {
            let highlights = result["highlights"]
                .as_array()
                .ok_or_else(|| bad("Exa result has no highlights"))?;
            let text = highlights
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| bad("Exa highlight is not text"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            (result.get("publishedDate"), text)
        }
        Engine::Tinyfish => (
            result.get("date"),
            http::text(result, "snippet")?.to_owned(),
        ),
    };
    Ok(json!({"title": title, "url": url, "published": date, "summary": summary}))
}
