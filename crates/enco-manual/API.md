<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_manual`

`src/lib.rs`

Markdown manuals rendered from rustdoc's JSON output.

An API manual says what a library offers its callers, by the paths they write. It names source
files but not lines, so only an interface change alters it. A navigation manual covers every
module and item, private ones included, with source lines, to show where to change what.

- struct `Problem` — One broken rule, with where the item is declared.
- enum `ReadError` — Why rustdoc's output could not be read.
- enum `Rule` — Which rule an item breaks.
- fn `api` — The API manual of a library: its public items by the paths callers write, with the source file each comes from, relative to `sources`.
- fn `doc_problems` — The rules one doc comment breaks, for docs written outside Rust such as the WIT's.
- fn `navigation` — The navigation manual of a crate, as Markdown files by name, `README.md` first.
- fn `problems` — The documentation rules a crate's items break, ordered by file and line.
- fn `read` — Read rustdoc's JSON output for one crate.
- const `RUSTDOCFLAGS` — The rustdoc flags that write the JSON the manuals read.
- const `SUMMARY_CHARS` — The longest first paragraph that still reads as one line of an index.

Re-exports `rustdoc_types::Crate`, `rustdoc_types::FORMAT_VERSION`.

## struct `Problem`

`src/rules.rs`

```rust
pub struct Problem {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub item: String,
    pub rule: Rule,
    pub message: String,
}
```

One broken rule, with where the item is declared.

- `file` — The source file, as rustdoc names it.
- `line` — The line the item starts on.
- `item` — The item's name.
- `rule` — The rule it breaks.
- `message` — What is wrong and what to do, in one sentence.

Implements:

```rust
impl Clone for Problem
impl Debug for Problem
```

## enum `ReadError`

`src/lib.rs`

```rust
pub enum ReadError {
    Json(serde_json::Error),
    Format {
        found: u32,
    },
}
```

Why rustdoc's output could not be read.

- `Json` — The output is not rustdoc JSON.
- `Format` — A toolchain other than the pinned one wrote it.
  - `found` — The format the file declares.

Implements:

```rust
impl Debug for ReadError
impl Display for ReadError
impl Error for ReadError
impl From<Error> for ReadError
```

## enum `Rule`

`src/rules.rs`

```rust
pub enum Rule {
    Unchecked,
    Waiver,
    Long,
    Restates,
    Placeholder,
}
```

Which rule an item breaks.

- `Unchecked` — The crate root does not turn on `missing_docs`, so the compiler lets undocumented items
  pass.
- `Waiver` — The item opts out of `missing_docs`; an item not worth documenting should not be public.
- `Long` — The first paragraph is longer than `SUMMARY_CHARS`.
- `Restates` — The first paragraph only repeats words of the item's name.
- `Placeholder` — The docs leave a placeholder for later work.

Implements:

```rust
impl Clone for Rule
impl Copy for Rule
impl Debug for Rule
impl Eq for Rule
impl PartialEq for Rule
```

## fn `api`

`src/lib.rs`

```rust
pub fn api(krate: &Crate, sources: &str) -> String
```

The API manual of a library: its public items by the paths callers write, with the source file
each comes from, relative to `sources`.

## fn `doc_problems`

`src/lib.rs`

```rust
pub fn doc_problems(name: &str, docs: &str) -> Vec<(Rule, String)>
```

The rules one doc comment breaks, for docs written outside Rust such as the WIT's.

`name` is the documented item's name. Each broken rule comes with what to do about it.

## fn `navigation`

`src/lib.rs`

```rust
pub fn navigation(krate: &Crate, sources: &str) -> std::collections::BTreeMap<String, String>
```

The navigation manual of a crate, as Markdown files by name, `README.md` first.

It needs rustdoc JSON that includes private items. Source lines are relative to `sources`.

## fn `problems`

`src/lib.rs`

```rust
pub fn problems(krate: &Crate) -> Vec<Problem>
```

The documentation rules a crate's items break, ordered by file and line.

## fn `read`

`src/lib.rs`

```rust
pub fn read(json: &[u8]) -> Result<Crate, ReadError>
```

Read rustdoc's JSON output for one crate.

## const `RUSTDOCFLAGS`

`src/lib.rs`

```rust
pub const RUSTDOCFLAGS: &str = "-Z unstable-options --output-format json --document-private-items --document-hidden-items";
```

The rustdoc flags that write the JSON the manuals read.

The JSON holds every item, private and hidden ones included: each manual chooses what it shows,
and the documentation rules see everything, including a waiver on a private module.

## const `SUMMARY_CHARS`

`src/rules.rs`

```rust
pub const SUMMARY_CHARS: usize = 160;
```

The longest first paragraph that still reads as one line of an index.
