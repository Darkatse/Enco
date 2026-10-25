use super::{InputSchema, canonical::canonical_schema};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::num::NonZeroU64;

const MAX_LINES: u64 = 2000;

fn first_line() -> u64 {
    1
}

fn all_lines() -> u64 {
    MAX_LINES
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "only the schema of this type is used")]
struct Arguments {
    /// File to read.
    path: String,
    /// First line to return, counting from 1.
    #[serde(default = "first_line")]
    #[schemars(range(min = 1))]
    offset: u64,
    /// Number of lines to return.
    #[serde(default = "all_lines")]
    #[schemars(range(min = 1, max = MAX_LINES))]
    limit: u64,
    /// Older side of a comparison.
    from: Option<Side>,
    /// Paths to include.
    paths: Option<Vec<String>>,
    /// Whether to include hidden files.
    #[serde(default)]
    hidden: bool,
    /// The order of the result.
    order: Option<Order>,
    /// How strictly to match.
    mode: Option<Mode>,
    /// The weight of a match.
    #[schemars(range(min = 0.5, max = 2))]
    weight: Option<f64>,
    /// Tags to match.
    #[schemars(length(max = 2))]
    tags: Option<Vec<String>>,
}

/// The order of a result.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    /// Newest first.
    Newest,
    /// Oldest first.
    Oldest,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Exact,
    Loose,
}

/// One side of a comparison.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
#[expect(dead_code, reason = "only the schema of this type is used")]
enum Side {
    /// A registration number.
    Registration(NonZeroU64),
    /// The files in the home.
    Home(Home),
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Home {
    Home,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

#[test]
fn generation_emits_the_canonical_form() {
    for (generated, parsed) in [
        (
            canonical_schema::<Arguments>(),
            InputSchema::of::<Arguments>(),
        ),
        (canonical_schema::<Nothing>(), InputSchema::of::<Nothing>()),
    ] {
        let parsed = parsed.unwrap();
        assert_eq!(parsed.to_value(), generated);
        assert_eq!(InputSchema::parse(&parsed.to_value()).unwrap(), parsed);
    }
    assert_eq!(InputSchema::of::<Nothing>().unwrap(), InputSchema::empty());
}

#[test]
fn declarations_outside_the_subset_fail_where_they_break_it() {
    let property = |schema: Value| json!({"type": "object", "additionalProperties": false, "properties": {"a": schema}});
    for (declared, expected) in [
        (
            json!({"type": "object"}),
            "schema root: an object must set `additionalProperties` to false",
        ),
        (
            json!({"type": "object", "additionalProperties": false, "description": "Reads."}),
            "schema root: the root takes no description or default; describe the tool in its description",
        ),
        (
            property(json!({"type": "string", "format": "date"})),
            "/properties/a: `format` is not supported for a string",
        ),
        (
            property(json!({"type": ["string", "null"]})),
            "/properties/a: `type` must name a single type",
        ),
        (
            property(json!({"oneOf": [{"type": "string"}, {"type": "integer"}]})),
            "/properties/a: `oneOf` is not supported",
        ),
        (
            property(json!({"type": "integer", "minimum": 1, "default": 0})),
            "/properties/a: `default` must be at least 1",
        ),
    ] {
        assert_eq!(
            InputSchema::parse(&declared).unwrap_err().to_string(),
            expected
        );
    }
}

#[test]
fn arguments_are_checked_by_path_and_null_means_not_given() {
    let schema = InputSchema::of::<Arguments>().unwrap();
    let check = |arguments: Value| {
        schema
            .accept(object(arguments))
            .map(Value::Object)
            .map_err(|error| error.to_string())
    };
    assert_eq!(
        check(
            json!({"path": "a", "offset": null, "limit": 5e0, "from": "home", "weight": 1.5, "mode": "exact"})
        ),
        Ok(json!({"path": "a", "limit": 5, "from": "home", "weight": 1.5, "mode": "exact"}))
    );
    for (arguments, expected) in [
        (
            json!({"path": "a", "limt": 5}),
            "unknown argument `limt`; the arguments are `from`, `hidden`, `limit`, `mode`, `offset`, `order`, `path`, `paths`, `tags`, `weight`",
        ),
        (json!({"path": null}), "`path` is required"),
        (
            json!({"path": "a", "limit": 999_999}),
            "`limit` must be at most 2000",
        ),
        (
            json!({"path": "a", "offset": 2.5}),
            "`offset` must be an integer",
        ),
        (
            json!({"path": "a", "offset": 9_007_199_254_740_993.0}),
            "`offset` must be an integer",
        ),
        (
            json!({"path": "a", "paths": ["b", 3]}),
            "`paths[1]` must be a string",
        ),
        (
            json!({"path": "a", "from": "workspace"}),
            "`from` must be an integer; or `from` must be one of \"home\"",
        ),
        (
            json!({"path": "a", "mode": "fuzzy"}),
            "`mode` must be one of \"exact\", \"loose\"",
        ),
        (
            json!({"path": "a", "order": "random"}),
            "`order` must be one of \"newest\"; or `order` must be one of \"oldest\"",
        ),
        (
            json!({"path": "a", "weight": 2.5}),
            "`weight` must be at most 2",
        ),
        (
            json!({"path": "a", "tags": ["a", "b", "c"]}),
            "`tags` must have at most 2 items",
        ),
    ] {
        assert_eq!(check(arguments), Err(expected.to_owned()));
    }
}
