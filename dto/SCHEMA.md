# The cast metadata sidecar

One JSON file per recording, next to it: `demo.cast` → `demo.meta.json`. It carries the three things a bare recording cannot: a **title**, a **summary**, and **timekeyed chapters**. All three are optional — a recording with no sidecar still plays.

The machine-checkable rendering lives in [`schema/beecast-meta.schema.json`](schema/beecast-meta.schema.json); the Rust types in this crate's [`src/lib.rs`](src/lib.rs) are the source of truth, and a unit test keeps the two in sync (the schema is generated from the types). [SeeCast](../seecast/README.md) — the annotator living in this repo — generates files in this exact shape, and its validator (`validate_meta` in [`seecast/seecast`](../seecast/seecast)) enforces the same rules on everything it emits.

```json
{
  "title": "Rebuilding the parser",
  "summary": "Replaces the ad-hoc tokenizer with a table-driven one and gets the suite green.",
  "chapters": [
    { "t": 0,     "title": "Setup" },
    { "t": 42.5,  "title": "First failing test" },
    { "t": 187.0, "title": "Green build" }
  ]
}
```

## Fields

| Field      | Type   | Meaning |
|------------|--------|---------|
| `title`    | string, optional | Short human title; the page's `<title>` and header. Falls back to the recording's filename. |
| `summary`  | string, optional | One or two sentences on what the recording shows; rendered under the title. |
| `chapters` | array, optional  | Chapter markers: timeline marks plus one navigation button each. |

Each chapter is `{ "t": <seconds>, "title": "<short phrase>" }`.

## The timekey `t`

- **Seconds into the recording** — not `mm:ss`, not milliseconds.
- **Fractional values are allowed** (`12.5` means twelve and a half seconds in).
- **The first chapter MUST start at exactly `0`** — the opening segment always has a marker, YouTube-style.
- Chapters are **strictly ascending** by `t`: no ties, no going back.

## Strictness

Parsing is strict end to end: an unknown key anywhere — a typo like `"titel"`, an extra field on a chapter — is a hard error, not a silently-ignored no-op. Empty strings are rejected too: omit a field instead of passing `""`. SeeCast applies the same strictness to the model's replies: extra keys, out-of-order chapters, or a missing summary mean no sidecar is written at all.
