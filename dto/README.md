# `beecast-dto`

The cast-metadata DTO for [BeeCast](https://github.com/dkorolev/beecast): the strongly-typed `{ title, summary, chapters }` sidecar that annotates an asciinema `.cast` recording with a title, a summary, and timekeyed chapters.

This crate is the **source of truth** for the metadata shape. The [`beecast`](https://crates.io/crates/beecast) CLI renders files in this shape, and the SeeCast annotator generates them; both agree because both go through this one definition.

```rust
use beecast_dto::{parse, CastMeta};

let meta: CastMeta = parse(r#"{ "title": "Demo", "chapters": [{ "t": 0, "title": "Start" }] }"#)?;
assert_eq!(meta.title.as_deref(), Some("Demo"));
```

## What it gives you

- **`CastMeta` / `Chapter`** — the sidecar types. Serde with `deny_unknown_fields`, so a typo is a hard error, not a silently-dropped key.
- **`parse`** — deserialize and validate in one step, returning a typed `ParseError`.
- **`CastMeta::validate`** — enforces the invariants JSON Schema can't express: non-empty strings, finite timekeys, strictly ascending chapters, and the first chapter pinned to `t = 0`.
- **`generated_schema`** — renders the formal JSON Schema *from these Rust types* (doc comments become descriptions). It is the codegen that produces [`schema/beecast-meta.schema.json`](schema/beecast-meta.schema.json); a unit test pins the shipped file byte-for-byte to that output, so the two can never drift.

The human-readable rendering of the shape is [`SCHEMA.md`](SCHEMA.md).

## License

MIT.
