//! History navigation and file comparison use the same accepted-intent query as registration.
use super::{Development, DevelopmentError, Failed, Operation, RewindArguments};
mod content;
mod diff;
use crate::status::{CallStep, Steps};
use content::Unit;
pub use content::{ContentFailure, ContentUnavailable};
use diff::compare_contents;
use enco_core::*;
use enco_kernel::{RegisteredIntent, RegistrationDetails, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const HISTORY_PAGE_SIZE: usize = 20;

const HOME: &str = "home";
const SIDE: &str = "a side is a registration number or \"home\"";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryPoint {
    Registration(RegistrationId),
    Home,
}

impl std::str::FromStr for HistoryPoint {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == HOME {
            Ok(Self::Home)
        } else {
            value
                .parse()
                .map(|id| Self::Registration(RegistrationId(id)))
                .map_err(|_| SIDE)
        }
    }
}

impl JsonSchema for HistoryPoint {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "HistoryPoint".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut registration = generator.subschema_for::<RegistrationId>();
        registration.insert("description".into(), "A registration number.".into());
        schemars::json_schema!({
            "anyOf": [
                registration,
                { "type": "string", "enum": [HOME], "description": "The files in the home." }
            ]
        })
    }
}

impl Serialize for HistoryPoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Registration(id) => serializer.serialize_u64(id.0),
            Self::Home => serializer.serialize_str(HOME),
        }
    }
}

impl<'de> Deserialize<'de> for HistoryPoint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Side;
        impl serde::de::Visitor<'_> for Side {
            type Value = HistoryPoint;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(SIDE)
            }
            fn visit_u64<E: serde::de::Error>(self, id: u64) -> Result<HistoryPoint, E> {
                Ok(HistoryPoint::Registration(RegistrationId(id)))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<HistoryPoint, E> {
                if value == HOME {
                    Ok(HistoryPoint::Home)
                } else {
                    Err(E::custom(SIDE))
                }
            }
        }
        deserializer.deserialize_any(Side)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "QueryFields", into = "QueryFields")]
pub enum HistoryQuery {
    List {
        before: Option<RegistrationId>,
    },
    /// Two intent values. Without `from`, the home is compared with the latest
    /// registration, and a registration with the one before it.
    Compare {
        from: Option<HistoryPoint>,
        to: HistoryPoint,
    },
}

/// The tool's argument object; a cursor and a comparison cannot be combined.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryFields {
    /// The side the diff starts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<HistoryPoint>,
    /// The side the diff ends at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<HistoryPoint>,
    /// List only the registrations older than this number; not used with `from` or `to`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<RegistrationId>,
}

impl JsonSchema for HistoryQuery {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "HistoryQuery".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        QueryFields::json_schema(generator)
    }
}

impl HistoryQuery {
    /// The one rule for the tool's and the CLI's arguments: a cursor lists, and a side compares.
    pub fn new(
        from: Option<HistoryPoint>,
        to: Option<HistoryPoint>,
        before: Option<RegistrationId>,
    ) -> Result<Self, &'static str> {
        match (from, to, before) {
            (None, None, before) => Ok(Self::List { before }),
            (from, to, None) => Ok(Self::Compare {
                from,
                to: to.unwrap_or(HistoryPoint::Home),
            }),
            (_, _, Some(_)) => Err("before pages the registration list; omit it when comparing"),
        }
    }
}

impl TryFrom<QueryFields> for HistoryQuery {
    type Error = &'static str;
    fn try_from(fields: QueryFields) -> Result<Self, Self::Error> {
        Self::new(fields.from, fields.to, fields.before)
    }
}

impl From<HistoryQuery> for QueryFields {
    fn from(query: HistoryQuery) -> Self {
        match query {
            HistoryQuery::List { before } => Self {
                from: None,
                to: None,
                before,
            },
            HistoryQuery::Compare { from, to } => Self {
                from,
                to: Some(to),
                before: None,
            },
        }
    }
}

#[derive(Serialize)]
pub struct History {
    pub registrations: Vec<RegistrationDetails>,
    /// The cursor for older registrations, when more exist.
    pub before: Option<RegistrationId>,
    pub comparison: Option<Comparison>,
    pub timezone: Tz,
}

#[derive(Serialize)]
pub struct Comparison {
    /// Absent only before the first registration.
    pub from: Option<HistoryPoint>,
    pub to: HistoryPoint,
    pub mode: ComparisonMode,
    pub units: Vec<UnitDiff>,
    /// Different identities would require the same current directory.
    pub conflicting_paths: Vec<IntentPath>,
    /// Actions include only paths addressable by the rewind tool.
    pub next: Option<Steps>,
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonMode {
    HomeChanges,
    RewindPreview,
    Registrations,
}

#[derive(Serialize)]
pub struct UnitDiff {
    pub path: IntentPath,
    pub plugin: Option<PluginId>,
    pub files: Vec<FileDiff>,
    pub unavailable: Option<ContentUnavailable>,
}

#[derive(Serialize)]
pub struct FileDiff {
    pub path: String,
    #[serde(flatten)]
    pub change: FileDifference,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileDifference {
    Text { diff: String },
    AddedEmpty,
    RemovedEmpty,
    Binary,
}

impl Development {
    /// Compare file values by plugin identity; names only locate their current paths.
    pub async fn history(
        &self,
        registry: &Registry,
        query: HistoryQuery,
    ) -> Result<History, Failed> {
        self.read_history(registry, query)
            .await
            .map_err(|error| Operation::History.failed(error))
    }

    async fn read_history(
        &self,
        registry: &Registry,
        query: HistoryQuery,
    ) -> Result<History, DevelopmentError> {
        let timezone = registry.view().config.kernel.timezone;
        let (from, to) = match query {
            HistoryQuery::List { before } => {
                let mut registrations = registry.history(before, HISTORY_PAGE_SIZE + 1).await?;
                let more = registrations.len() > HISTORY_PAGE_SIZE;
                registrations.truncate(HISTORY_PAGE_SIZE);
                return Ok(History {
                    before: more
                        .then(|| registrations.last().map(|entry| entry.id))
                        .flatten(),
                    registrations,
                    comparison: None,
                    timezone,
                });
            }
            HistoryQuery::Compare { from, to } => (from, to),
        };
        let latest = registry.registered_intent(None).await?;
        let latest_id = latest.accepted()?.id;
        let mut registrations = Vec::new();
        let from = match (from, to) {
            (Some(from), _) => Some(from),
            (None, HistoryPoint::Home) => Some(HistoryPoint::Registration(latest_id)),
            (None, HistoryPoint::Registration(id)) => {
                let detail = registry.registration(id).await?;
                let from = detail.previous.map(HistoryPoint::Registration);
                registrations.push(detail);
                from
            }
        };
        let names = match (from, to) {
            (Some(HistoryPoint::Home), _) | (_, HistoryPoint::Home) => latest.clone(),
            (_, HistoryPoint::Registration(id)) => registry.registered_intent(Some(id)).await?,
        };
        let comparison = self
            .comparison(registry, from, to, &names, &latest, latest_id)
            .await?;
        Ok(History {
            registrations,
            before: None,
            comparison: Some(comparison),
            timezone,
        })
    }

    async fn comparison(
        &self,
        registry: &Registry,
        from: Option<HistoryPoint>,
        to: HistoryPoint,
        names: &RegisteredIntent,
        latest: &RegisteredIntent,
        latest_id: RegistrationId,
    ) -> Result<Comparison, DevelopmentError> {
        let before = match from {
            Some(point) => self.contents(registry, point, names).await?,
            None => BTreeMap::new(),
        };
        let after = self.contents(registry, to, names).await?;
        let compared = compare_contents(before, after);
        let target = match (from, to) {
            (Some(HistoryPoint::Home), HistoryPoint::Registration(id))
            | (Some(HistoryPoint::Registration(id)), HistoryPoint::Home) => Some(id),
            _ => None,
        };
        let (rewind, conflicting_paths) = match target {
            Some(id) => {
                let target = registry.registered_intent(Some(id)).await?;
                let (paths, conflicting) = rewind_paths(&compared, latest, &target);
                ((!paths.is_empty()).then_some((id, paths)), conflicting)
            }
            None => (None, Vec::new()),
        };
        let mode =
            if to == HistoryPoint::Home && from == Some(HistoryPoint::Registration(latest_id)) {
                ComparisonMode::HomeChanges
            } else if from == Some(HistoryPoint::Home) {
                ComparisonMode::RewindPreview
            } else {
                ComparisonMode::Registrations
            };
        let next = match (&mode, rewind) {
            (ComparisonMode::HomeChanges, rewind)
                if compared.values().any(|diff| diff.unavailable.is_none()) =>
            {
                Some(Steps {
                    first: vec![CallStep::Register.into()],
                    alternative: rewind
                        .map(|(latest, paths)| vec![CallStep::Discard { latest, paths }.into()]),
                })
            }
            (_, rewind) => rewind.map(|(registration, paths)| {
                Steps::sequence(vec![
                    CallStep::Rewind(RewindArguments {
                        registration,
                        paths: Some(paths),
                    })
                    .into(),
                ])
            }),
        };
        Ok(Comparison {
            from,
            to,
            mode,
            units: compared.into_values().collect(),
            conflicting_paths,
            next,
        })
    }
}

/// The paths a rewind to the target addresses, as decided by the one rewind scope, and the
/// directories that two plugins would need at once.
fn rewind_paths(
    compared: &BTreeMap<Unit, UnitDiff>,
    latest: &RegisteredIntent,
    target: &RegisteredIntent,
) -> (Vec<IntentPath>, Vec<IntentPath>) {
    let scope = latest.rewind_scope(target);
    let available = compared
        .iter()
        .filter(|(_, diff)| diff.unavailable.is_none());
    let plugins: Vec<_> = available
        .clone()
        .filter_map(|(unit, _)| match unit {
            Unit::Plugin(id) => Some(*id),
            Unit::Configuration | Unit::Unregistered(_) => None,
        })
        .collect();
    let collisions = scope.collisions(plugins.iter().copied());
    let mut paths: Vec<_> = available
        .filter(|(unit, _)| matches!(unit, Unit::Configuration))
        .map(|_| IntentPath::Configuration)
        .collect();
    paths.extend(
        plugins
            .iter()
            .filter_map(|id| scope.addressable(*id))
            .filter(|directory| !collisions.contains(*directory))
            .cloned()
            .map(IntentPath::Plugin),
    );
    let conflicting = collisions.into_iter().map(IntentPath::Plugin).collect();
    (paths, conflicting)
}
