use enco_core::GenerationId;
use enco_kernel::{
    Endpoint, KernelSettings, Need, SettingsError, TableOwner, Use, View, parse_table,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

/// Configured embedding caller. The adapter is resolved for each synchronization call.
#[derive(Clone)]
pub(super) struct EmbeddingEndpoint {
    pub endpoint: Endpoint,
    /// Expected vector width, part of the derived index identity.
    pub dimensions: NonZeroUsize,
}

impl EmbeddingEndpoint {
    /// All request facts that may change the vector space; credentials and labels do not.
    pub fn identity(&self) -> serde_json::Value {
        serde_json::json!({ "base_url": self.endpoint.settings.base_url, "model": self.endpoint.settings.model, "options": self.endpoint.settings.options, "dimensions": self.dimensions })
    }
}

/// Memory's choices of named model endpoints.
pub(super) struct MemorySettings {
    pub embedding: EmbeddingEndpoint,
    pub relevance: Option<Endpoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    embedding: String,
    relevance: Option<String>,
}

impl MemorySettings {
    fn from_document(document: &Value, kernel: &KernelSettings) -> Result<Self, SettingsError> {
        let raw: RawSettings = parse_table(document, "memory")?;
        let embed = kernel.endpoint(&raw.embedding, "memory")?;
        let dimensions = embed.dimensions.ok_or_else(|| {
            SettingsError::new(
                "memory",
                format!("endpoint {} needs dimensions", raw.embedding),
            )
        })?;
        let embedding = EmbeddingEndpoint {
            endpoint: embed.endpoint.clone(),
            dimensions,
        };
        let relevance = raw
            .relevance
            .map(|name| {
                kernel
                    .endpoint(&name, "memory")
                    .map(|spec| spec.endpoint.clone())
            })
            .transpose()?;
        Ok(Self {
            embedding,
            relevance,
        })
    }

    fn uses(&self) -> Vec<Use> {
        let embed = &self.embedding;
        let mut uses = vec![Use {
            user: "memory.embedding".into(),
            endpoint: embed.endpoint.clone(),
            need: Need::Embedding {
                dimensions: embed.dimensions,
            },
        }];
        if let Some(endpoint) = &self.relevance {
            uses.push(Use {
                user: "memory.relevance".into(),
                endpoint: endpoint.clone(),
                need: Need::Decision,
            });
        }
        uses
    }
}

/// The same parser supplies admission and runtime consumers.
pub const TABLE_OWNER: TableOwner = TableOwner {
    tables: &[enco_kernel::ConfigTable::Single("memory")],
    declare: |document, kernel| Ok(MemorySettings::from_document(document, kernel)?.uses()),
};

#[derive(Default)]
pub(super) struct SettingsCache(Mutex<Option<Cached>>);
struct Cached {
    generation: GenerationId,
    settings: Result<Arc<MemorySettings>, SettingsError>,
}

impl SettingsCache {
    pub fn get(&self, view: &View) -> Result<Arc<MemorySettings>, SettingsError> {
        let mut cached = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = &*cached
            && cached.generation == view.config.generation
        {
            return cached.settings.clone();
        }
        let settings =
            MemorySettings::from_document(&view.config.document, &view.config.kernel).map(Arc::new);
        *cached = Some(Cached {
            generation: view.config.generation,
            settings: settings.clone(),
        });
        settings
    }
}
