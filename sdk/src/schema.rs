//! Tool argument schemas generated from Rust types, in the canonical subset of JSON Schema that the
//! host accepts.
//!
//! The host compiles this file as well, so host tools and plugins describe arguments the same way.
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::{Value, json};

/// The canonical argument schema of `T`.
///
/// Generation drops what the subset leaves out: titles, formats, the root description (a tool's
/// description has its own field) and the `null` that `Option` adds, since an absent property
/// already means "not given".
pub fn canonical_schema<T: JsonSchema>() -> Value {
    let mut schema = SchemaSettings::draft2020_12()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    if let Value::Object(root) = &mut schema {
        root.remove("description");
    }
    canonicalize(&mut schema);
    schema
}

/// The part of a doc comment the model reads: without a `# Implementation` section, which is for
/// maintainers, and with soft line breaks joined, as in Markdown.
fn model_text(text: &str) -> String {
    text.lines()
        .take_while(|line| line.trim() != "# Implementation")
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .split("\n\n")
        .map(|paragraph| {
            paragraph
                .split('\n')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Rewrite one schema node and the schema nodes below it; property names and data stay as they are.
fn canonicalize(schema: &mut Value) {
    let Value::Object(node) = schema else { return };
    node.remove("title");
    node.remove("format");
    if let Some(Value::String(text)) = node.get_mut("description") {
        *text = model_text(text);
    }
    if node.get("default") == Some(&Value::Null) {
        node.remove("default");
    }
    // A documented enum variant becomes `{"const": v}` inside `oneOf`; its variants never overlap.
    if let Some(value) = node.remove("const") {
        node.insert("enum".into(), json!([value]));
    }
    if let Some(branches) = node.remove("oneOf") {
        node.insert("anyOf".into(), branches);
    }
    if let Some(Value::Array(types)) = node.get_mut("type") {
        types.retain(|kind| *kind != "null");
    }
    if let Some(Value::Array(types)) = node.get("type")
        && let [kind] = types.as_slice()
    {
        let kind = kind.clone();
        node.insert("type".into(), kind);
    }
    if let Some(Value::Array(values)) = node.get_mut("enum") {
        values.retain(|value| !value.is_null());
    }
    if let Some(Value::Array(branches)) = node.get_mut("anyOf") {
        branches.retain(|branch| {
            branch.get("type").is_none_or(|kind| *kind != "null")
                && branch.get("const").is_none_or(|value| !value.is_null())
        });
    }
    if let Some(Value::Array(branches)) = node.get("anyOf")
        && let [Value::Object(only)] = branches.as_slice()
    {
        // The field's own description outranks the description of the type it wraps.
        let only = only.clone();
        node.remove("anyOf");
        for (keyword, value) in only {
            node.entry(keyword).or_insert(value);
        }
        canonicalize(schema);
        return;
    }
    if node.get("type").is_some_and(|kind| kind == "object") {
        // The canonical form always writes both, as the services have always received them.
        node.entry("properties").or_insert_with(|| json!({}));
        node.entry("required").or_insert_with(|| json!([]));
    }
    if let Some(Value::Object(properties)) = node.get_mut("properties") {
        properties.values_mut().for_each(canonicalize);
    }
    if let Some(items) = node.get_mut("items") {
        canonicalize(items);
    }
    if let Some(Value::Array(branches)) = node.get_mut("anyOf") {
        branches.iter_mut().for_each(canonicalize);
    }
}
