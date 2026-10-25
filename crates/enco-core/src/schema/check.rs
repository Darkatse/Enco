//! Checking a call's arguments against a parsed schema; messages name the argument by its path,
//! such as `limit`, `filter.name` or `paths[2]`.
use super::{Bounds, Count, Object, Schema, Shape};
use serde_json::{Map, Number, Value};
use std::cmp::Ordering;

impl Object {
    pub(super) fn check(
        &self,
        arguments: &mut Map<String, Value>,
        path: &str,
    ) -> Result<(), String> {
        if let Some(name) = arguments
            .keys()
            .find(|name| !self.properties.contains_key(*name))
        {
            let names: Vec<_> = self
                .properties
                .keys()
                .map(|name| format!("`{name}`"))
                .collect();
            let known = if names.is_empty() {
                "it takes none".to_owned()
            } else {
                format!("the arguments are {}", names.join(", "))
            };
            return Err(format!("unknown argument `{}`; {known}", join(path, name)));
        }
        arguments.retain(|name, value| !value.is_null() || self.required.contains(name));
        for name in &self.required {
            if arguments.get(name).is_none_or(Value::is_null) {
                return Err(format!("`{}` is required", join(path, name)));
            }
        }
        for (name, value) in arguments.iter_mut() {
            if let Some(schema) = self.properties.get(name) {
                schema.check(value, &join(path, name))?;
            }
        }
        Ok(())
    }
}

impl Schema {
    pub(super) fn check(&self, value: &mut Value, path: &str) -> Result<(), String> {
        let expected = |kind: &str| format!("`{path}` must be {kind}");
        match (&self.shape, value) {
            (Shape::Object(object), Value::Object(arguments)) => object.check(arguments, path),
            (Shape::Object(_), _) => Err(expected("an object")),
            (Shape::String { values, length }, Value::String(text)) => {
                if let Some(values) = values
                    && !values.contains(text)
                {
                    let values: Vec<_> =
                        values.iter().map(|value| format!("\"{value}\"")).collect();
                    return Err(format!("`{path}` must be one of {}", values.join(", ")));
                }
                length.check(text.chars().count(), path, "characters")
            }
            (Shape::String { .. }, _) => Err(expected("a string")),
            (Shape::Integer(bounds), Value::Number(number))
                if let Some(integer) = whole(number) =>
            {
                *number = integer;
                bounds.check(number, path)
            }
            (Shape::Integer(_), _) => Err(expected("an integer")),
            (Shape::Number(bounds), Value::Number(number)) => bounds.check(number, path),
            (Shape::Number(_), _) => Err(expected("a number")),
            (Shape::Boolean, Value::Bool(_)) => Ok(()),
            (Shape::Boolean, _) => Err(expected("a boolean")),
            (Shape::Array { items, count }, Value::Array(list)) => {
                count.check(list.len(), path, "items")?;
                for (index, item) in list.iter_mut().enumerate() {
                    items.check(item, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            (Shape::Array { .. }, _) => Err(expected("an array")),
            (Shape::AnyOf(branches), value) => {
                let mut problems = Vec::new();
                for branch in branches {
                    let mut candidate = value.clone();
                    match branch.check(&mut candidate, path) {
                        Ok(()) => {
                            *value = candidate;
                            return Ok(());
                        }
                        Err(reason) => problems.push(reason),
                    }
                }
                Err(problems.join("; or "))
            }
        }
    }
}

impl Count {
    fn check(&self, actual: usize, path: &str, unit: &str) -> Result<(), String> {
        let actual = actual as u64;
        if let Some(low) = self.min
            && actual < low
        {
            return Err(format!("`{path}` must have at least {low} {unit}"));
        }
        if let Some(high) = self.max
            && actual > high
        {
            return Err(format!("`{path}` must have at most {high} {unit}"));
        }
        Ok(())
    }
}

impl Bounds {
    fn check(&self, number: &Number, path: &str) -> Result<(), String> {
        if let Some(low) = &self.minimum
            && compare(number, low) == Ordering::Less
        {
            return Err(format!("`{path}` must be at least {low}"));
        }
        if let Some(high) = &self.maximum
            && compare(number, high) == Ordering::Greater
        {
            return Err(format!("`{path}` must be at most {high}"));
        }
        Ok(())
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

/// The integer a number denotes, written without a fraction: as in JSON Schema, any number with a
/// zero fractional part is an integer, so `5.0` and `5e0` are `5`.
///
/// A float of 2^53 or more may have lost digits when it was read, so it is not taken for an integer.
pub(super) fn whole(number: &Number) -> Option<Number> {
    match number.as_f64() {
        Some(value) if number.is_f64() => (value.fract() == 0.0 && value.abs() < 2f64.powi(53))
            .then(|| Number::from(value as i64)),
        _ => Some(number.clone()),
    }
}

pub(super) fn compare(left: &Number, right: &Number) -> Ordering {
    let exact = |number: &Number| {
        number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from))
    };
    match (exact(left), exact(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => {
            let float = |number: &Number| number.as_f64().unwrap_or_default();
            float(left).total_cmp(&float(right))
        }
    }
}
