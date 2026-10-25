//! Ratewell's v1 API, which quotes the ECB reference rates against any base currency.
use crate::failure;
use enco_sdk::{host, types::Failure};
use serde::Deserialize;
use std::collections::BTreeMap;

const API: &str = "http://api.ratewell.io/v1";

/// The latest rates against one base currency.
#[derive(Deserialize)]
pub(crate) struct Latest {
    pub(crate) base: String,
    /// The ECB publication date, `YYYY-MM-DD`.
    pub(crate) date: String,
    /// How many units of each currency one unit of `base` buys.
    pub(crate) rates: BTreeMap<String, f64>,
}

impl Latest {
    pub(crate) fn rate(&self, currency: &str) -> Result<f64, Failure> {
        self.rates.get(currency).copied().ok_or_else(|| {
            failure(
                "tool.failed",
                format!("Ratewell publishes no {currency} rate"),
            )
        })
    }
}

/// The latest rates against `base`, limited to `symbols` unless it is empty.
pub(crate) async fn latest(base: &str, symbols: &[String]) -> Result<Latest, Failure> {
    let mut url = format!("{API}/latest?base={base}");
    if !symbols.is_empty() {
        url.push_str("&symbols=");
        url.push_str(&symbols.join(","));
    }
    let response = host::http(host::HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![("accept".into(), "application/json".into())],
        body: None,
        timeout_ms: None,
    })
    .await
    .map_err(|error| Failure {
        code: "tool.failed".into(),
        message: format!("Ratewell is unreachable: {}", error.message),
        retryable: true,
    })?;
    if !(200..300).contains(&response.status) {
        return Err(Failure {
            code: "tool.failed".into(),
            message: format!(
                "Ratewell answered HTTP {}: {}",
                response.status,
                String::from_utf8_lossy(&response.body).trim()
            ),
            retryable: response.status == 429 || response.status >= 500,
        });
    }
    serde_json::from_slice(&response.body).map_err(|error| {
        failure(
            "tool.failed",
            format!("Ratewell's response did not parse: {error}"),
        )
    })
}
