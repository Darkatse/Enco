use anyhow::{Context, Result, bail};
use enco_core::{DEFAULT_PROFILE, ProviderSettings, Tz};
use enco_host::{DecisionEndpoint, EmbeddingEndpoint};
use enco_kernel::{Budget, Endpoint, Interface, Profile, Use};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

/// Resolved once at startup; routing and invocation share these same values.
pub(crate) struct Config {
    pub timezone: Tz,
    pub profiles: BTreeMap<String, Profile>,
    pub embedding: EmbeddingEndpoint,
    pub decision: Option<DecisionEndpoint>,
    pub run: RunConfig,
    pub telegram: Option<TelegramConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    timezone: Tz,
    endpoint: BTreeMap<String, EndpointConfig>,
    profile: BTreeMap<String, ProfileConfig>,
    embedding: EmbeddingConfig,
    decision: Option<DecisionConfig>,
    #[serde(default)]
    run: RunConfig,
    telegram: Option<TelegramConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointConfig {
    plugin: String,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    #[serde(default = "empty_options")]
    options: serde_json::Value,
    window_tokens: u32,
    max_output_tokens: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingConfig {
    plugin: String,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    #[serde(default = "empty_options")]
    options: serde_json::Value,
    dimensions: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionConfig {
    plugin: String,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    #[serde(default = "empty_options")]
    options: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileConfig {
    reply: String,
    compaction: String,
    #[serde(default = "requires_lifeline")]
    requires_lifeline: bool,
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

fn empty_options() -> serde_json::Value {
    serde_json::json!({})
}

fn requires_lifeline() -> bool {
    true
}

impl Config {
    pub async fn load(path: &Path) -> Result<Self> {
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("cannot read {}; run `enco init` first", path.display()))?;
        let raw: RawConfig = toml::from_str(&text).context("invalid config.toml")?;
        if !raw.profile.contains_key(DEFAULT_PROFILE) {
            bail!("[profile.{DEFAULT_PROFILE}] is required");
        }
        let endpoints = raw
            .endpoint
            .into_iter()
            .map(|(name, endpoint)| {
                endpoint
                    .resolve()
                    .with_context(|| format!("[endpoint.{name}]"))
                    .map(|endpoint| (name, endpoint))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut profiles = BTreeMap::new();
        for (name, profile) in raw.profile {
            let endpoint = |purpose: &str, reference: &str| {
                endpoints.get(reference).cloned().with_context(|| {
                    format!("[profile.{name}].{purpose}: unknown endpoint {reference}")
                })
            };
            profiles.insert(
                name.clone(),
                Profile {
                    reply: endpoint("reply", &profile.reply)?,
                    compaction: endpoint("compaction", &profile.compaction)?,
                    requires_lifeline: profile.requires_lifeline,
                },
            );
        }
        Ok(Self {
            timezone: raw.timezone,
            profiles,
            embedding: raw.embedding.resolve().context("[embedding]")?,
            decision: raw
                .decision
                .map(DecisionConfig::resolve)
                .transpose()
                .context("[decision]")?,
            run: raw.run,
            telegram: raw.telegram,
        })
    }

    pub fn wiring(&self) -> Vec<Use> {
        let mut wiring = Vec::new();
        for (name, profile) in &self.profiles {
            for (purpose, endpoint) in [
                ("reply", &profile.reply),
                ("compaction", &profile.compaction),
            ] {
                wiring.push(Use {
                    user: format!("profile {name}.{purpose}"),
                    plugin: endpoint.plugin.clone(),
                    interface: Interface::Completion,
                });
            }
        }
        wiring.push(Use {
            user: "embedding".into(),
            plugin: self.embedding.plugin.clone(),
            interface: Interface::Embedding,
        });
        if let Some(endpoint) = &self.decision {
            wiring.push(Use {
                user: "decision".into(),
                plugin: endpoint.plugin.clone(),
                interface: Interface::Decision,
            });
        }
        wiring
    }
}

impl EndpointConfig {
    fn resolve(self) -> Result<Endpoint> {
        if self.window_tokens == 0
            || self.max_output_tokens == 0
            || self.max_output_tokens >= self.window_tokens
        {
            bail!(
                "window_tokens and max_output_tokens must be positive, with output smaller than the window"
            );
        }
        let (settings, api_key) = resolve_settings(ProviderSettings {
            base_url: self.base_url,
            model: self.model,
            api_key_env: self.api_key_env,
            options: self.options,
        })?;
        Ok(Endpoint {
            plugin: self.plugin,
            settings,
            api_key,
            budget: Budget {
                context_tokens: self.window_tokens,
                max_output_tokens: self.max_output_tokens,
            },
        })
    }
}

impl EmbeddingConfig {
    fn resolve(self) -> Result<EmbeddingEndpoint> {
        if self.dimensions == 0 {
            bail!("dimensions must be positive");
        }
        let (settings, api_key) = resolve_settings(ProviderSettings {
            base_url: self.base_url,
            model: self.model,
            api_key_env: self.api_key_env,
            options: self.options,
        })?;
        Ok(EmbeddingEndpoint {
            plugin: self.plugin,
            settings,
            api_key,
            dimensions: self.dimensions,
        })
    }
}

impl DecisionConfig {
    fn resolve(self) -> Result<DecisionEndpoint> {
        let (settings, api_key) = resolve_settings(ProviderSettings {
            base_url: self.base_url,
            model: self.model,
            api_key_env: self.api_key_env,
            options: self.options,
        })?;
        Ok(DecisionEndpoint {
            plugin: self.plugin,
            settings,
            api_key,
        })
    }
}

fn resolve_settings(settings: ProviderSettings) -> Result<(ProviderSettings, Option<String>)> {
    if settings.base_url.trim().is_empty() || settings.model.trim().is_empty() {
        bail!("base_url and model must not be empty");
    }
    let api_key = settings
        .api_key_env
        .as_ref()
        .map(|name| {
            std::env::var(name)
                .with_context(|| format!("API key environment variable {name} is not set"))
        })
        .transpose()?;
    Ok((settings, api_key))
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
