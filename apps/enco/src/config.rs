use anyhow::{Context, Result, bail};
use enco_core::ProviderSettings;
use enco_host::EmbeddingEndpoint;
use enco_kernel::Profile;
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
    pub telegram: Option<TelegramConfig>,
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
    pub async fn load(path: &Path) -> Result<(Self, Profile, EmbeddingEndpoint)> {
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("cannot read {}; run `enco init` first", path.display()))?;
        let config: Self = toml::from_str(&text).context("invalid config.toml")?;
        let reply = config.provider.resolve()?;
        let embedding = config.embedding.resolve()?;
        if config.provider.dimensions.is_some() {
            bail!("dimensions belongs in [embedding], not [provider]");
        }
        let dimensions = config
            .embedding
            .dimensions
            .filter(|dimensions| *dimensions > 0)
            .context("[embedding].dimensions must be positive")?;
        let embedding = EmbeddingEndpoint {
            plugin: embedding.plugin,
            settings: embedding.settings,
            api_key: embedding.api_key,
            dimensions,
        };
        let profile = Profile {
            compaction: reply.clone(),
            reply,
        };
        Ok((config, profile, embedding))
    }
}

impl Endpoint {
    fn resolve(&self) -> Result<enco_kernel::Endpoint> {
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
        Ok(enco_kernel::Endpoint {
            plugin: self.plugin.clone(),
            settings: ProviderSettings {
                base_url: self.base_url.clone(),
                model: self.model.clone(),
                api_key_env: self.api_key_env.clone(),
                options: self.options.clone(),
            },
            api_key,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelegramConfig {
    pub token_env: String,
    pub owner_user_id: u64,
    pub api_base: Option<String>,
}

impl TelegramConfig {
    pub fn adapter(&self) -> Result<enco_host::Telegram> {
        if self.owner_user_id == 0 {
            bail!("[telegram].owner_user_id must be positive");
        }
        let token = std::env::var(&self.token_env).with_context(|| {
            format!(
                "bot token environment variable {} is not set",
                self.token_env
            )
        })?;
        Ok(enco_host::Telegram::new(
            token,
            self.api_base
                .as_deref()
                .unwrap_or("https://api.telegram.org"),
        )?)
    }
}
