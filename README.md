# `beecast`

In-browser `.cast` files player, with chapters and seeing capabilities.

Also designed to be self-contained, so that when the file is saved, it's a single HTML, and it can be viewed totally offline.

**beecast** turns an [asciinema](https://asciinema.org) `.cast` recording into a **single, fully self-contained `.html` file** that plays in any browser — online, offline, from a web server, or straight off a `file://` path. Zero network requests, zero external dependencies, zero console errors or warnings. If you `Save` the page from a browser and open the copy on a plane, it works exactly the same.

## What's in this repo

A Cargo workspace of four published crates, plus a companion annotator:

| Directory | Crate / tool | What it is |
| --- | --- | --- |
| [`dto/`](dto) | `beecast-dto` (crates.io) | The cast-metadata DTO — the `{ title, summary, chapters }` sidecar types, their validator, and the JSON Schema generated from them. The source of truth for the metadata shape. |
| [`player/`](player) | `beecast-player` (crates.io) | The first-party clean-room player as a crate — the asciicast player and VT emulator, exposed as inlinable JS/CSS constants, with live-follow `append` for recordings that are still growing. |
| [`page/`](page) | `beecast-page` (crates.io) | The page pipeline as a library with **nothing third-party** — cast inspection plus the renderer that turns cast text and plain-strings metadata into the self-contained `.html`. Embeds the player from `beecast-player`. |
| [`cli/`](cli) | `beecast` (crates.io) | The CLI that renders a `.cast` (plus optional sidecar) into one self-contained `.html`. Depends on `beecast-dto` and `beecast-page`. |
| [`seecast/`](seecast) | `seecast` (repo tool) | The AI annotator (single-file, stdlib-only Python) that writes a sidecar from a recording, via `cursor-agent`. |

The metadata schema is the shared contract: `beecast-dto` defines it in Rust, `beecast` renders files in its shape, and `seecast` generates them. One schema, one source of truth.

## Quick start

```
cargo install beecast          # the CLI, from crates.io
beecast build demo.cast        # → demo.html, next to the cast; play it fully offline
```

Full CLI docs are in [`cli/README.md`](cli/README.md); the metadata shape in [`dto/SCHEMA.md`](dto/SCHEMA.md); the annotator in [`seecast/README.md`](seecast/README.md).

## Building and publishing

```
cargo test --workspace && cargo test --workspace --release
```

The crates publish to crates.io in dependency order — `beecast-dto` and `beecast-player` first (they are independent of each other), then `beecast-page`, then `beecast` — as documented in [`PUBLISHING.md`](PUBLISHING.md). Contribution and gate details are in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

beecast is MIT (see [`LICENSE`](LICENSE)) — all of it. The pages embed beecast's own clean-room player, `beecast-player` (written from scratch against the asciicast format and ECMA-48/xterm documentation; see [`player/README.md`](player/README.md)), so no third-party code — and no second license — ships in the crates, the binary, or any generated page.
