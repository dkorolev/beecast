# `beecast-page`

The [BeeCast](https://github.com/dkorolev/beecast) page pipeline as a library: turn asciinema `.cast` text plus plain-strings metadata into one **fully self-contained `.html` player page** — the player, its styles, the recording, and the metadata all inlined, zero network requests, so a saved copy keeps working fully offline.

This crate has **zero dependencies** — deliberately. Its API takes plain strings and floats, never serde types, so a consumer with a tiny dependency tree (`scsh`, which hand-rolls its JSON on purpose, is the motivating one) embeds the whole pipeline without pulling serde, anyhow, or anything else transitively. The few JSON needs are met by an internal std-only module whose behavior — and output bytes — match the serde-backed renderer this crate was extracted from; the `beecast` CLI test suite pins that equivalence differentially against serde itself.

```rust
use beecast_page::{build_page, inspect, PageMeta};

let cast = std::fs::read_to_string("demo.cast")?;
let info = inspect(&cast)?; // asciicast v1/v2/v3, plus the duration when it is cheap to know
let meta = PageMeta { title: Some("Demo"), summary: None, chapters: &[(0.0, "Start"), (12.5, "Mid")] };
let html = build_page(&cast, &meta, "demo.cast");
```

## What it gives you

- **`build_page`** — cast text + `PageMeta` + a fallback title (the recording's filename) → the final HTML string. Hostile input cannot break out of the page: titles and summaries are HTML-escaped, and the recording and metadata are embedded `<script>`-safe (every `<` is neutralized to the `\u003c` escape, so no `</script>` in a recording can terminate the script early).
- **`inspect`** — light validation of the recording: reject a non-asciicast early with a typed `CastError`, tell v1/v2/v3 apart, and compute the total duration when that is cheap.
- **`PageMeta`** — the borrowed plain-strings metadata shape: `title`, `summary`, and `(seconds, title)` chapter pairs.

Validating the metadata (strictly ascending chapters, the first at `t = 0`) stays the caller's job: the [`beecast`](https://crates.io/crates/beecast) CLI validates through [`beecast-dto`](https://crates.io/crates/beecast-dto) and converts to plain strings at this crate's boundary. What the generated page can do — chapter navigation, 0.5×–3× speed, `?t=<seconds>&note=<comment>` deep links — is documented in the [CLI README](https://github.com/dkorolev/beecast/blob/main/cli/README.md).

## License

BeeCast is MIT (text in [`LICENSE`](LICENSE), shipped with the crate) — all of it. The inlined player is BeeCast's own clean-room `scsh-cast-player` (see [`src/player/README.md`](src/player/README.md)), so no third-party code or license ships in the crate or in any generated page.
