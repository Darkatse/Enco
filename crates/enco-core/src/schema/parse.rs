//! Parsing a declared schema into the canonical subset; every rejection names its location.
use super::{Bounds, Count, Object, Schema, SchemaError, Shape, check, problem};
use serde_json::Value;
use std::{cmp::Ordering, collections::BTreeMap};

type Keywords<'a> = BTreeMap<&'a str, &'a Value>;

/// Every keyword the canonical subset uses somewhere.
const SUBSET: [&str; 15] = [
    "type",
    "description",
    "default",
    "anyOf",
    "properties",
    "required",
    "additionalProperties",
    "enum",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "items",
    "minItems",
    "maxItems",
];

pub(super) fn schema(value: &Value, at: &str) -> Result<Schema, SchemaError> {
    let Value::Object(node) = value else {
        return Err(problem(at, "a schema must be a JSON object"));
    };
    let mut keywords: Keywords<'_> = node
        .iter()
        .map(|(name, value)| (name.as_str(), value))
        .collect();
    let description = match keywords.remove("description") {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => return Err(problem(at, "`description` must be a string")),
    };
    let default = keywords.remove("default").cloned();
    let (shape, kind) = shape(&mut keywords, at)?;
    if let Some(keyword) = keywords.keys().next() {
        return Err(problem(
            at,
            format!("`{keyword}` is not supported for {kind}"),
        ));
    }
    let mut schema = Schema {
        description,
        default: None,
        shape,
    };
    if let Some(mut default) = default {
        schema
            .check(&mut default, "default")
            .map_err(|reason| problem(at, reason))?;
        schema.default = Some(default);
    }
    Ok(schema)
}

/// The shape a schema's `type` or `anyOf` declares, with what it is called in messages.
fn shape(keywords: &mut Keywords<'_>, at: &str) -> Result<(Shape, &'static str), SchemaError> {
    let kind = match keywords.remove("type") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(_) => return Err(problem(at, "`type` must name a single type")),
        None => {
            return match keywords.remove("anyOf") {
                Some(Value::Array(branches)) if branches.len() >= 2 => Ok((
                    Shape::AnyOf(
                        branches
                            .iter()
                            .enumerate()
                            .map(|(index, branch)| schema(branch, &format!("{at}/anyOf/{index}")))
                            .collect::<Result<_, _>>()?,
                    ),
                    "anyOf",
                )),
                Some(_) => Err(problem(at, "`anyOf` must list at least two schemas")),
                None => Err(
                    match keywords.keys().find(|keyword| !SUBSET.contains(keyword)) {
                        Some(keyword) => problem(at, format!("`{keyword}` is not supported")),
                        None => problem(at, "a schema needs `type` or `anyOf`"),
                    },
                ),
            };
        }
    };
    Ok(match kind {
        "object" => (Shape::Object(Object::parse(keywords, at)?), "an object"),
        "string" => (
            Shape::String {
                values: values(keywords, at)?,
                length: Count::parse(keywords, at, "minLength", "maxLength")?,
            },
            "a string",
        ),
        "integer" => (
            Shape::Integer(Bounds::parse(keywords, at, true)?),
            "an integer",
        ),
        "number" => (
            Shape::Number(Bounds::parse(keywords, at, false)?),
            "a number",
        ),
        "boolean" => (Shape::Boolean, "a boolean"),
        "array" => {
            let Some(items) = keywords.remove("items") else {
                return Err(problem(at, "an array must declare `items`"));
            };
            (
                Shape::Array {
                    items: Box::new(schema(items, &format!("{at}/items"))?),
                    count: Count::parse(keywords, at, "minItems", "maxItems")?,
                },
                "an array",
            )
        }
        other => return Err(problem(at, format!("type `{other}` is not supported"))),
    })
}

fn values(keywords: &mut Keywords<'_>, at: &str) -> Result<Option<Vec<String>>, SchemaError> {
    let Some(values) = keywords.remove("enum") else {
        return Ok(None);
    };
    let invalid = || problem(at, "`enum` must list one or more strings");
    match values {
        Value::Array(values) if !values.is_empty() => values
            .iter()
            .map(|value| value.as_str().map(str::to_owned).ok_or_else(invalid))
            .collect::<Result<_, _>>()
            .map(Some),
        _ => Err(invalid()),
    }
}

impl Object {
    fn parse(keywords: &mut Keywords<'_>, at: &str) -> Result<Self, SchemaError> {
        if keywords.remove("additionalProperties") != Some(&Value::Bool(false)) {
            return Err(problem(
                at,
                "an object must set `additionalProperties` to false",
            ));
        }
        let properties = match keywords.remove("properties") {
            None => BTreeMap::new(),
            Some(Value::Object(properties)) => properties
                .iter()
                .map(|(name, value)| {
                    let pointer = name.replace('~', "~0").replace('/', "~1");
                    Ok((
                        name.clone(),
                        schema(value, &format!("{at}/properties/{pointer}"))?,
                    ))
                })
                .collect::<Result<_, SchemaError>>()?,
            Some(_) => return Err(problem(at, "`properties` must be an object")),
        };
        let invalid = || problem(at, "`required` must list property names");
        let required: Vec<String> = match keywords.remove("required") {
            None => Vec::new(),
            Some(Value::Array(names)) => names
                .iter()
                .map(|name| name.as_str().map(str::to_owned).ok_or_else(invalid))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(invalid()),
        };
        for (index, name) in required.iter().enumerate() {
            if !properties.contains_key(name) {
                return Err(problem(
                    at,
                    format!("`required` names `{name}`, which is not a property"),
                ));
            }
            if required[..index].contains(name) {
                return Err(problem(at, format!("`required` lists `{name}` twice")));
            }
        }
        Ok(Self {
            properties,
            required,
        })
    }
}

impl Count {
    fn parse(
        keywords: &mut Keywords<'_>,
        at: &str,
        min: &str,
        max: &str,
    ) -> Result<Self, SchemaError> {
        let mut limit = |keyword: &str| match keywords.remove(keyword) {
            None => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| problem(at, format!("`{keyword}` must be a non-negative integer"))),
        };
        let count = Self {
            min: limit(min)?,
            max: limit(max)?,
        };
        if let (Some(low), Some(high)) = (count.min, count.max)
            && low > high
        {
            return Err(problem(at, format!("`{min}` is greater than `{max}`")));
        }
        Ok(count)
    }
}

impl Bounds {
    fn parse(keywords: &mut Keywords<'_>, at: &str, integer: bool) -> Result<Self, SchemaError> {
        let expected = if integer { "an integer" } else { "a number" };
        let mut bound = |keyword: &str| {
            let Some(value) = keywords.remove(keyword) else {
                return Ok(None);
            };
            match value {
                Value::Number(number) if integer => check::whole(number),
                Value::Number(number) => Some(number.clone()),
                _ => None,
            }
            .map(Some)
            .ok_or_else(|| problem(at, format!("`{keyword}` must be {expected}")))
        };
        let bounds = Self {
            minimum: bound("minimum")?,
            maximum: bound("maximum")?,
        };
        if let (Some(low), Some(high)) = (&bounds.minimum, &bounds.maximum)
            && check::compare(low, high) == Ordering::Greater
        {
            return Err(problem(at, "`minimum` is greater than `maximum`"));
        }
        Ok(bounds)
    }
}
