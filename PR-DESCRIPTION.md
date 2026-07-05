## Summary
- Ship BeeCast: a Rust CLI that turns an asciinema `.cast` into a single self-contained `.html` page with offline playback, chapters, and deep links — plus SeeCast, a stdlib-only Python annotator that writes the metadata sidecar via `cursor-agent`.
- Establish one shared metadata contract (`beecast-dto`) as the source of truth, with a generated JSON Schema, cross-language validation, and crates.io-ready publishing for both crates.
- Gate the repo with matching pre-push hooks and CI (`fmt`, `clippy`, Rust tests, seecast unit tests) so the renderer and annotator stay aligned as the stack grows.

## What This Changes
BeeCast is a new tool chain for terminal recordings. The `beecast` CLI reads a `.cast` file (and an optional `{title, summary, chapters}` sidecar) and emits one fully inlined HTML file: vendored asciinema-player assets, cast data, and metadata are embedded so the page works offline from `file://` with no network requests. The page supports chapter navigation, playback speed controls, and shareable deep links via query parameters (`?t=` and `&note=`).

SeeCast is the companion annotator. Given a recording, it builds a compact transcript, calls `cursor-agent` headless on the Composer Fast model, validates the reply against the same schema `beecast` consumes, and writes `demo.meta.json` next to `demo.cast`. It is packaged as an `scsh` skill (`.skills/seecast/`) with a symlinked `./seecast` entry point for local use.

The repo is organized as a Cargo workspace: `beecast-dto` owns the metadata types and schema generation; `beecast` is the renderer crate; `seecast/` is the Python tool. Both CLIs follow the house §2 contract — structured `--json` output, staged errors, documented exit codes, interrupt handling — with documented waivers where the tools' one-shot nature makes full §3/§5 coverage unnecessary.

## How to Verify

- **Deterministic gate** (same as CI and pre-push): `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo test --workspace --release && python3 -m unittest discover -s seecast/tests`.
- **The generated page, in a browser**: `cargo run -p beecast -- build cli/tests/fixtures/sample.cast -o /tmp/sample.html`, open it, and check chapter seeking, mid-playback speed switching, a `?t=1&note=hi` deep link parking the player with the note banner, and a silent network tab (the page is fully self-contained).
- **The real annotator path** (needs `cursor-agent` logged in): `./seecast/seecast <recording.cast>` on a real recording — liveness ticks on stderr every ~10 s, then the sidecar passes `./seecast/seecast --validate <recording>.meta.json`.
- **Publish readiness**: `cargo publish -p beecast-dto --dry-run` verifies the DTO tarball; `cargo package -p beecast --list` shows the CLI crate's contents (the full CLI dry run needs `beecast-dto` on crates.io first — see `PUBLISHING.md`).

## Implementation Details
- **Rust / `beecast-dto` (`dto/`)**: `CastMeta` and `Chapter` types with `deny_unknown_fields`, `parse` + `validate` (ascending chapters, first at `t = 0`, non-empty strings). `generated_schema()` renders `dto/schema/beecast-meta.schema.json` from the types; a unit test pins the shipped file byte-for-byte.
- **Rust / `beecast` CLI (`cli/`)**: `build` command inlines vendored asciinema-player v3.17.0 (Apache-2.0, `MIT AND Apache-2.0` crate license), embeds cast + optional sidecar into `page.html` via `include_str!`, and renders title/summary/chapters with a flex-based player layout. Integration tests cover CLI behavior, schema output, and inline-safe vendor bundle properties. `schema` and `help exitcodes` subcommands expose the public machine-mode surface.
- **Python / SeeCast (`seecast/`, `.skills/seecast/`)**: Single-file stdlib script handles asciicast v2/v3 transcript extraction (ANSI-stripped, deduplicated), `cursor-agent -p` invocation with watchdog timeout and one retry on stall, strict sidecar validation before write, and §2 JSON documents (`Annotated`, `Valid`, `Version`, `Error` with `stage`). Python style is gated by the same `rustfmt.toml` column/tab rules via `test_style.py`.
- **Schema & docs**: Human-readable shape in `dto/SCHEMA.md`; workspace-level `README.md`, `CONTRIBUTING.md`, `PUBLISHING.md`, and per-crate READMEs document install, gates, and the `beecast-dto` → `beecast` publish order.
- **CI & hooks**: `.githooks/pre-push` and `.github/workflows/ci.yml` run the same checks — `cargo fmt --check`, `cargo clippy --workspace`, `cargo test` (debug + release), and `python3 -m unittest discover -s seecast/tests`.
