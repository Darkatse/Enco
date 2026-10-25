//! The canonical subset of JSON Schema that tool arguments are declared in, and the check that
//! dispatch applies to every call.
//!
//! A declaration is parsed once, when it enters; anything outside the subset fails with its
//! location. Recorded plans keep schemas as they were sent and are never parsed again.
#[path = "../../../sdk/src/schema.rs"]
mod canonical;
mod check;
mod parse;
#[cfg(test)]
mod tests;

use schemars::JsonSchema;
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

/// A tool's argument schema: a closed object in the canonical subset that the plugin contract
/// states for `tool-spec`.
#[derive(Debug, Clone, PartialEq)]
pub struct InputSchema(Object);

/// Why a declared schema is outside the canonical subset, and where, as a JSON Pointer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {problem}", place(.location))]
pub struct SchemaError {
    location: String,
    problem: String,
}

/// Why a call's arguments do not fit the tool's schema, naming the argument.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ArgumentError(String);

#[derive(Debug, Clone, PartialEq)]
struct Schema {
    description: Option<String>,
    default: Option<Value>,
    shape: Shape,
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Object(Object),
    String {
        values: Option<Vec<String>>,
        length: Count,
    },
    Integer(Bounds),
    Number(Bounds),
    Boolean,
    Array {
        items: Box<Schema>,
        count: Count,
    },
    AnyOf(Vec<Schema>),
}

#[derive(Debug, Clone, PartialEq)]
struct Object {
    properties: BTreeMap<String, Schema>,
    required: Vec<String>,
}

/// Inclusive limits on a string's characters or an array's items.
#[derive(Debug, Clone, PartialEq)]
struct Count {
    min: Option<u64>,
    max: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Bounds {
    minimum: Option<Number>,
    maximum: Option<Number>,
}

impl InputSchema {
    /// Parse a declared schema. The root must be a closed object without a description or default:
    /// the tool's description has its own field.
    pub fn parse(value: &Value) -> Result<Self, SchemaError> {
        match parse::schema(value, "")? {
            Schema {
                description: None,
                default: None,
                shape: Shape::Object(object),
            } => Ok(Self(object)),
            Schema {
                shape: Shape::Object(_),
                ..
            } => Err(problem(
                "",
                "the root takes no description or default; describe the tool in its description",
            )),
            _ => Err(problem("", "the root must be an object")),
        }
    }

    /// The schema of `T`, generated from the type and its field documentation.
    pub fn of<T: JsonSchema>() -> Result<Self, SchemaError> {
        Self::parse(&canonical::canonical_schema::<T>())
    }

    /// The schema of a tool that takes no arguments.
    pub fn empty() -> Self {
        Self(Object {
            properties: BTreeMap::new(),
            required: Vec::new(),
        })
    }

    /// The schema in canonical JSON form.
    pub fn to_value(&self) -> Value {
        let mut node = Map::new();
        self.0.write(&mut node);
        Value::Object(node)
    }

    /// Check a call's arguments and return them in canonical form: optional properties given as
    /// `null` removed, since `null` there means "not given", and integers such as `5.0` as `5`.
    pub fn accept(
        &self,
        mut arguments: Map<String, Value>,
    ) -> Result<Map<String, Value>, ArgumentError> {
        self.0.check(&mut arguments, "").map_err(ArgumentError)?;
        Ok(arguments)
    }
}

fn place(location: &str) -> &str {
    if location.is_empty() {
        "schema root"
    } else {
        location
    }
}

fn problem(location: &str, problem: impl Into<String>) -> SchemaError {
    SchemaError {
        location: location.into(),
        problem: problem.into(),
    }
}

impl Schema {
    fn to_value(&self) -> Value {
        let mut node = Map::new();
        if let Some(description) = &self.description {
            node.insert("description".into(), description.clone().into());
        }
        if let Some(default) = &self.default {
            node.insert("default".into(), default.clone());
        }
        match &self.shape {
            Shape::Object(object) => object.write(&mut node),
            Shape::String { values, length } => {
                node.insert("type".into(), "string".into());
                if let Some(values) = values {
                    node.insert("enum".into(), values.clone().into());
                }
                length.write(&mut node, "minLength", "maxLength");
            }
            Shape::Integer(bounds) => {
                node.insert("type".into(), "integer".into());
                bounds.write(&mut node);
            }
            Shape::Number(bounds) => {
                node.insert("type".into(), "number".into());
                bounds.write(&mut node);
            }
            Shape::Boolean => {
                node.insert("type".into(), "boolean".into());
            }
            Shape::Array { items, count } => {
                node.insert("type".into(), "array".into());
                node.insert("items".into(), items.to_value());
                count.write(&mut node, "minItems", "maxItems");
            }
            Shape::AnyOf(branches) => {
                node.insert(
                    "anyOf".into(),
                    branches.iter().map(Schema::to_value).collect(),
                );
            }
        }
        Value::Object(node)
    }
}

impl Object {
    fn write(&self, node: &mut Map<String, Value>) {
        node.insert("type".into(), "object".into());
        node.insert(
            "properties".into(),
            self.properties
                .iter()
                .map(|(name, schema)| (name.clone(), schema.to_value()))
                .collect::<Map<_, _>>()
                .into(),
        );
        node.insert("required".into(), self.required.clone().into());
        node.insert("additionalProperties".into(), false.into());
    }
}

impl Count {
    fn write(&self, node: &mut Map<String, Value>, min: &str, max: &str) {
        if let Some(low) = self.min {
            node.insert(min.into(), low.into());
        }
        if let Some(high) = self.max {
            node.insert(max.into(), high.into());
        }
    }
}

impl Bounds {
    fn write(&self, node: &mut Map<String, Value>) {
        if let Some(low) = &self.minimum {
            node.insert("minimum".into(), low.clone().into());
        }
        if let Some(high) = &self.maximum {
            node.insert("maximum".into(), high.clone().into());
        }
    }
}
