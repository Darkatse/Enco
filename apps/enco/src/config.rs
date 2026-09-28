use anyhow::{Context, Result, bail};
use enco_host::EmbeddingSpec;
use enco_wasm::ProviderSettings;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub provider: Endpoint,
    pub embedding: Endpoint,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub run: RunConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Endpoint {
    #[serde(default = "default_plugin")]
    pub plugin: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    #[serde(default = "empty_options")]
    pub options: serde_json::Value,
    pub dimensions: Option<usize>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ContextConfig {
    pub window_tokens: u32,
    pub max_output_tokens: u32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            window_tokens: 128000,
            max_output_tokens: 8192,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RunConfig {
    pub max_rounds: u32,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { max_rounds: 24 }
    }
}

fn default_plugin() -> String {
    "openai-compatible".into()
}

fn empty_options() -> serde_json::Value {
    serde_json::json!({})
}

impl Config {
    pub async fn load(path: &Path) -> Result<(Self, EmbeddingSpec)> {
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("cannot read {}; run `enco init` first", path.display()))?;
        let config: Self = toml::from_str(&text).context("invalid config.toml")?;
        for endpoint in [&config.provider, &config.embedding] {
            endpoint.settings()?;
        }
        if config.provider.dimensions.is_some() {
            bail!("dimensions belongs in [embedding], not [provider]");
        }
        let dimensions = config
            .embedding
            .dimensions
            .filter(|dimensions| *dimensions > 0)
            .context("[embedding].dimensions must be positive")?;
        let embedding = EmbeddingSpec {
            model: config.embedding.model.clone(),
            dimensions,
        };
        Ok((config, embedding))
    }
}

impl Endpoint {
    pub fn settings(&self) -> Result<ProviderSettings> {
        if self.base_url.trim().is_empty() || self.model.trim().is_empty() {
            bail!("provider base_url and model must not be empty");
        }
        let api_key = self
            .api_key_env
            .as_ref()
            .map(|name| {
                std::env::var(name)
                    .with_context(|| format!("API key environment variable {name} is not set"))
            })
            .transpose()?;
        Ok(ProviderSettings {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            api_key,
            options: self.options.clone(),
        })
    }
}
