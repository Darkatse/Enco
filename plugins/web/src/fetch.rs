use crate::{
    bad,
    config::{Config, Engine, Service},
    http, invalid,
    types::Failure,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Arguments of `web_fetch`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    /// HTTP or HTTPS URL of the page to read.
    url: String,
}

pub(crate) async fn fetch(config: &Config, input: &str) -> Result<String, Failure> {
    let input: Arguments = enco_sdk::parse_arguments(input)?;
    let url = url::Url::parse(&input.url).map_err(|error| invalid(format!("url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid("url must use http or https"));
    }
    let response = match http::request(
        "GET",
        format!("{}/{}", config.url(Service::Jina), input.url),
        None,
    )
    .await
    {
        Ok(response) => http::decode(&response)?,
        Err(failure) => return fallback(config, &input.url, failure).await,
    };
    let data = &response["data"];
    if let Some(status) = data["httpStatus"].as_u64()
        && status >= 400
    {
        return fallback(
            config,
            &input.url,
            bad(format!("page returned HTTP {status}")),
        )
        .await;
    }
    let content = http::text(data, "content")?;
    let mut output = format!("Source: Jina Reader\nURL: {}\n\n{content}", input.url);
    if let Some(warning) = data["warning"].as_str() {
        output.push_str(&format!("\n\nWarning: {warning}"));
    }
    Ok(output)
}

async fn fallback(config: &Config, url: &str, failure: Failure) -> Result<String, Failure> {
    if !config.search.contains(&Engine::Exa) {
        return Err(failure);
    }
    exa(config, url, &failure.message).await.map_err(|second| {
        bad(format!(
            "Jina Reader: {}; Exa: {}",
            failure.message, second.message
        ))
    })
}

async fn exa(config: &Config, url: &str, reason: &str) -> Result<String, Failure> {
    let response = http::json(
        "POST",
        format!("{}/contents", config.url(Service::Exa)),
        Some(json!({"urls": [url], "text": true})),
    )
    .await?;
    if let Some(status) = response["statuses"]
        .as_array()
        .and_then(|items| items.first())
        && status["status"] != "success"
    {
        return Err(bad(format!(
            "content retrieval failed: {}",
            http::excerpt(&status.to_string())
        )));
    }
    let result = response["results"]
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| bad("Exa returned no page"))?;
    let content = http::text(result, "text")?;
    Ok(format!(
        "Source: Exa\nJina Reader failed: {reason}\nURL: {url}\n\n{content}"
    ))
}
