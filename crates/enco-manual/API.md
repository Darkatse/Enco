<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_manual`

`src/lib.rs`

Markdown manuals rendered from rustdoc's JSON output.

An API manual says what a library offers its callers, by the paths they write. It names source
files but not lines, so only an interface change alters it. A navigation manual covers every
module and item, private ones included, with source lines, to show where to change what.

- enum `ReadError` — Why rustdoc's output could not be read.
- fn `api` — The API manual of a library: its public items by the paths callers write, with the source file each comes from, relative to `sources`.
- fn `navigation` — The navigation manual of a crate, from rustdoc JSON that includes private items: file names mapped to Markdown, `README.md` first among them, with source lines relative to `sources`.
- fn `read` — Read rustdoc's JSON output for one crate.

Re-exports `rustdoc_types::Crate`, `rustdoc_types::FORMAT_VERSION`.

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

Implements `Debug`, `Display`, `Error`, `From<Error>`.

## fn `api`

`src/lib.rs`

```rust
pub fn api(krate: &Crate, sources: &str) -> String
```

The API manual of a library: its public items by the paths callers write, with the source file
each comes from, relative to `sources`.

## fn `navigation`

`src/lib.rs`

```rust
pub fn navigation(krate: &Crate, sources: &str) -> std::collections::BTreeMap<String, String>
```

The navigation manual of a crate, from rustdoc JSON that includes private items: file names
mapped to Markdown, `README.md` first among them, with source lines relative to `sources`.

## fn `read`

`src/lib.rs`

```rust
pub fn read(json: &[u8]) -> Result<Crate, ReadError>
```

Read rustdoc's JSON output for one crate.
