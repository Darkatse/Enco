//! Convert currencies at the euro foreign exchange reference rates that Ratewell republishes.
//!
//! The ECB publishes these rates once per working day, around 16:00 CET; on weekends and holidays
//! the latest rates are the previous working day's.
mod ratewell;

use bindings::exports::enco::plugin::{lifecycle, tools};
use enco_sdk::types;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

enco_sdk::export!(tools, Plugin);

struct Plugin;

/// Arguments of `fx_convert`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Convert {
    /// The amount to convert, in the `from` currency.
    amount: f64,
    /// ISO 4217 code of the currency the amount is in, such as `USD`.
    from: String,
    /// ISO 4217 code of the currency to convert into, such as `CNY`.
    to: String,
}

/// Arguments of `fx_rates`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Rates {
    /// ISO 4217 code of the currency the rates are quoted against.
    #[serde(default = "euro")]
    base: String,
    /// Currencies to list, such as `["USD", "JPY"]`; every published currency when empty.
    #[serde(default)]
    symbols: Vec<String>,
}

fn euro() -> String {
    "EUR".into()
}

impl lifecycle::Guest for Plugin {
    fn describe(config: String) -> Result<lifecycle::Description, types::Failure> {
        if config != "{}" {
            return Err(failure("plugin.config", "fx has no configuration fields"));
        }
        Ok(lifecycle::Description {
            summary: "Convert currencies at the ECB reference rates".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: Vec::new(),
        })
    }

    async fn probe() -> Result<(), types::Failure> {
        Ok(())
    }
}

impl tools::Guest for Plugin {
    fn list(_config: String) -> Vec<types::ToolSpec> {
        vec![
            types::ToolSpec {
                name: "fx_convert".into(),
                description: "Convert an amount between two currencies at the latest ECB reference rates. Returns the rate, the converted amount rounded to two decimals, and the date the rates were published.".into(),
                input_schema: enco_sdk::input_schema::<Convert>().to_string(),
                effect: types::Effect::ReadOnly,
            },
            types::ToolSpec {
                name: "fx_rates".into(),
                description: "List the latest ECB reference rates against a base currency, with the date they were published.".into(),
                input_schema: enco_sdk::input_schema::<Rates>().to_string(),
                effect: types::Effect::ReadOnly,
            },
        ]
    }

    async fn call(name: String, input: String) -> types::Outcome {
        let result = match name.as_str() {
            "fx_convert" => convert(&input).await,
            "fx_rates" => rates(&input).await,
            _ => Err(failure(
                "tool.invalid_arguments",
                format!("Unknown tool {name}"),
            )),
        };
        match result {
            Ok(value) => types::Outcome::Ok(value.to_string()),
            Err(failure) => types::Outcome::Failed(failure),
        }
    }
}

async fn convert(input: &str) -> Result<Value, types::Failure> {
    let request: Convert = enco_sdk::parse_arguments(input)?;
    let from = currency(&request.from)?;
    let to = currency(&request.to)?;
    let latest = ratewell::latest(&from, std::slice::from_ref(&to)).await?;
    let rate = latest.rate(&to)?;
    Ok(json!({
        "amount": request.amount,
        "from": from,
        "to": to,
        "rate": rate,
        "result": round(request.amount * rate, 2),
        "published": latest.date,
    }))
}

async fn rates(input: &str) -> Result<Value, types::Failure> {
    let request: Rates = enco_sdk::parse_arguments(input)?;
    let base = currency(&request.base)?;
    let symbols = request
        .symbols
        .iter()
        .map(|symbol| currency(symbol))
        .collect::<Result<Vec<_>, _>>()?;
    let latest = ratewell::latest(&base, &symbols).await?;
    Ok(json!({
        "base": latest.base,
        "published": latest.date,
        "rates": latest.rates,
    }))
}

/// An ISO 4217 code in upper case; Ratewell answers anything else with a bare 400.
fn currency(code: &str) -> Result<String, types::Failure> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        Ok(code)
    } else {
        Err(failure(
            "tool.invalid_arguments",
            format!("{code:?} is not a three-letter currency code"),
        ))
    }
}

fn round(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale).round() / scale
}

fn failure(code: &str, message: impl Into<String>) -> types::Failure {
    types::Failure {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}
