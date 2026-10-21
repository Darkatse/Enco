//! The kernel-owned configuration tables and their declared uses.
use crate::{Budget, CompletionEndpoint, Endpoints, Need, Profile, Use};
use enco_core::{DEFAULT_PROFILE, Modality, PluginName, ProviderSettings, Tz};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU32, NonZeroUsize},
};

/// Parsed kernel tables, sampled by each consumer at its own boundary.
#[derive(Clone)]
pub struct KernelSettings {
    /// Owner's civil timezone.
    pub timezone: Tz,
    /// Maximum number of Rounds in one Run.
    pub max_rounds_per_run: NonZeroU32,
    /// Named model services and their facts.
    endpoints: BTreeMap<String, EndpointSpec>,
    /// Named Session policies, including `default`.
    pub profiles: BTreeMap<String, Profile>,
}

/// A resolved model service, independent of its caller and code generation.
#[derive(Clone)]
pub struct Endpoint {
    /// Name of the configured model service.
    pub name: String,
    /// Registered plugin name whose exports serve this endpoint.
    pub plugin: PluginName,
    /// Durable parameters copied into AttemptStarted.
    pub settings: ProviderSettings,
}

/// A named model service; each user checks the facts it needs.
#[derive(Clone)]
pub struct EndpointSpec {
    /// Resolved name, plugin and invocation parameters.
    pub endpoint: Endpoint,
    /// Model's context window, if known.
    pub window_tokens: Option<NonZeroU32>,
    /// Model's output limit, if known.
    pub max_output_tokens: Option<NonZeroU32>,
    /// Embedding width, if applicable.
    pub dimensions: Option<NonZeroUsize>,
    /// Non-text input capabilities declared by the owner.
    pub accepts: BTreeSet<Modality>,
}

/// A configuration error attributed to the table that owns it.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("[{table}]: {message}")]
pub struct SettingsError {
    /// Owning table, including a name for endpoint and profile entries.
    pub table: String,
    /// Invalid field or relationship.
    pub message: String,
}

impl SettingsError {
    /// Attribute a parsing or validation failure to its table.
    pub fn new(table: impl Into<String>, message: impl ToString) -> Self {
        Self {
            table: table.into(),
            message: message.to_string(),
        }
    }
}

/// Parse one owner's table without interpreting other tables.
pub fn parse_table<T: DeserializeOwned>(document: &Value, table: &str) -> Result<T, SettingsError> {
    serde_json::from_value(document.get(table).cloned().unwrap_or(Value::Null))
        .map_err(|error| SettingsError::new(table, error))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    timezone: Tz,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Run {
    max_rounds: NonZeroU32,
}
impl Default for Run {
    fn default() -> Self {
        Self {
            max_rounds: const { NonZeroU32::new(24).unwrap() },
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEndpoint {
    plugin: PluginName,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    #[serde(default = "empty_options")]
    options: Value,
    window_tokens: Option<NonZeroU32>,
    max_output_tokens: Option<NonZeroU32>,
    dimensions: Option<NonZeroUsize>,
    #[serde(default)]
    accepts: BTreeSet<Modality>,
}

fn empty_options() -> Value {
    serde_json::json!({})
}
fn requires_lifeline() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    reply: Vec<String>,
    compaction: Vec<String>,
    #[serde(default = "requires_lifeline")]
    requires_lifeline: bool,
}

/// Named endpoint entries: `[endpoint.<name>]`.
pub const ENDPOINT_TABLE: crate::ConfigTable = crate::ConfigTable::Named("endpoint");
/// Named profile entries: `[profile.<name>]`.
pub const PROFILE_TABLE: crate::ConfigTable = crate::ConfigTable::Named("profile");

impl KernelSettings {
    /// Table ownership used by the registry's declaration evaluation.
    pub const TABLES: [crate::ConfigTable; 4] = [
        crate::ConfigTable::Single("owner"),
        crate::ConfigTable::Single("run"),
        ENDPOINT_TABLE,
        PROFILE_TABLE,
    ];

    /// Parse and validate only the kernel-owned tables.
    pub fn from_document(document: &Value) -> Result<Self, SettingsError> {
        let owner: Owner = parse_table(document, "owner")?;
        let run: Run = parse_table::<Option<Run>>(document, "run")?.unwrap_or_default();
        let raw: BTreeMap<String, Value> = parse_table(document, ENDPOINT_TABLE.name())?;
        let mut endpoints = BTreeMap::new();
        for (name, value) in raw {
            let table = format!("{}.{name}", ENDPOINT_TABLE.name());
            let raw: RawEndpoint =
                serde_json::from_value(value).map_err(|error| SettingsError::new(&table, error))?;
            if raw.base_url.trim().is_empty() || raw.model.trim().is_empty() {
                return Err(SettingsError::new(
                    table,
                    "base_url and model must not be empty",
                ));
            }
            endpoints.insert(
                name.clone(),
                EndpointSpec {
                    endpoint: Endpoint {
                        name,
                        plugin: raw.plugin,
                        settings: ProviderSettings {
                            base_url: raw.base_url,
                            model: raw.model,
                            api_key_env: raw.api_key_env,
                            options: raw.options,
                        },
                    },
                    window_tokens: raw.window_tokens,
                    max_output_tokens: raw.max_output_tokens,
                    dimensions: raw.dimensions,
                    accepts: raw.accepts,
                },
            );
        }
        let raw: BTreeMap<String, Value> = parse_table(document, PROFILE_TABLE.name())?;
        if !raw.contains_key(DEFAULT_PROFILE) {
            return Err(SettingsError::new(
                format!("{}.{DEFAULT_PROFILE}", PROFILE_TABLE.name()),
                "required for new Sessions",
            ));
        }
        let mut settings = Self {
            timezone: owner.timezone,
            max_rounds_per_run: run.max_rounds,
            endpoints,
            profiles: BTreeMap::new(),
        };
        for (name, value) in raw {
            let table = format!("{}.{name}", PROFILE_TABLE.name());
            let raw: RawProfile =
                serde_json::from_value(value).map_err(|error| SettingsError::new(&table, error))?;
            let endpoints = |purpose: &str, names: &[String]| {
                let (first, rest) = names.split_first().ok_or_else(|| {
                    SettingsError::new(
                        &table,
                        format!("{purpose} must contain at least one endpoint"),
                    )
                })?;
                Ok::<_, SettingsError>(Endpoints::new(
                    settings.completion(first, &table)?,
                    rest.iter()
                        .map(|name| settings.completion(name, &table))
                        .collect::<Result<_, _>>()?,
                ))
            };
            settings.profiles.insert(
                name,
                Profile {
                    reply: endpoints("reply", &raw.reply)?,
                    compaction: endpoints("compaction", &raw.compaction)?,
                    requires_lifeline: raw.requires_lifeline,
                },
            );
        }
        Ok(settings)
    }

    /// Resolve a model service and attribute lookup failures to its user's table.
    pub fn endpoint(&self, name: &str, table: &str) -> Result<&EndpointSpec, SettingsError> {
        self.endpoints
            .get(name)
            .ok_or_else(|| SettingsError::new(table, format!("unknown endpoint {name}")))
    }

    /// Apply the profile's completion requirements to the endpoint's declared facts.
    fn completion(&self, name: &str, table: &str) -> Result<CompletionEndpoint, SettingsError> {
        let spec = self.endpoint(name, table)?;
        let (Some(context_tokens), Some(max_output_tokens)) =
            (spec.window_tokens, spec.max_output_tokens)
        else {
            return Err(SettingsError::new(
                table,
                format!("endpoint {name} needs window_tokens and max_output_tokens"),
            ));
        };
        if max_output_tokens >= context_tokens {
            return Err(SettingsError::new(
                table,
                format!("endpoint {name}: output must be smaller than the window"),
            ));
        }
        Ok(CompletionEndpoint {
            endpoint: spec.endpoint.clone(),
            accepts: spec.accepts.clone(),
            budget: Budget {
                context_tokens,
                max_output_tokens,
            },
        })
    }

    /// Project each profile's requirements from its parsed endpoints.
    pub fn uses(&self) -> Vec<Use> {
        self.profiles
            .iter()
            .flat_map(|(name, profile)| {
                [
                    ("reply", &profile.reply, true, true),
                    ("compaction", &profile.compaction, false, false),
                ]
                .into_iter()
                .flat_map(move |(purpose, endpoints, tools, media)| {
                    endpoints.iter().map(move |endpoint| Use {
                        user: format!("profile {name}.{purpose}"),
                        endpoint: endpoint.endpoint.clone(),
                        need: Need::Completion {
                            tools,
                            max_output_tokens: endpoint.budget.max_output_tokens,
                            accepts: if media {
                                endpoint.accepts.clone()
                            } else {
                                BTreeSet::new()
                            },
                        },
                    })
                })
            })
            .collect()
    }
}
