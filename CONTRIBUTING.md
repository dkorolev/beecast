# Contributing

A Cargo workspace with five parts: the [`beecast-dto`](dto) crate (`dto/` — the cast-metadata DTO and the source of truth for the schema), the [`beecast-player`](player) crate (`player/` — the first-party clean-room player and VT emulator as inlinable JS/CSS constants), the [`beecast-page`](page) crate (`page/` — the page pipeline: cast inspection and the HTML renderer, embedding the player crate), the [`beecast`](cli) CLI crate (`cli/` — argument parsing and I/O, depends on the rest), and the Python `seecast` annotator (`seecast/`). The version is shared once, in the root `[workspace.package]`. Crates publish in dependency order — see [`PUBLISHING.md`](PUBLISHING.md).

`dto/schema/beecast-meta.schema.json` is *generated* from the Rust types in `dto/src/lib.rs` (the source of truth) — regenerate with `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`; a unit test in `beecast-dto` pins the shipped file byte-for-byte, and a Python test cross-checks the facts `validate_meta` mirrors, so drift dies in the gate.

Two gates, same checks (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --workspace --release`, `python3 -m unittest discover -s seecast/tests`, and a **warning-free** `cargo package --workspace` — packaging is the dry run of publishing, and a warning there means the published crates would silently differ from the repo's intent):

1. **Pre-push** — enable once per clone: `git config core.hooksPath .githooks`. Committing locally stays free and fast; the gate runs when code leaves the machine.
2. **CI** — `.github/workflows/ci.yml`, required before merge.

One deliberate deviation from "pre-push and CI run the same gates": the browser tests in `browser-tests/` are **CI-required but locally optional** — a three-browser Playwright matrix is too heavy for pre-push. CI builds the fixture page with the real CLI (`cargo run -p beecast -- build`) and asserts, in chromium, firefox, and webkit, over both `file://` and a local HTTP server, that the page makes zero network requests after the initial load and emits zero console errors or warnings through a playback session. Run locally with `npm install && npx playwright install && npx playwright test` from `browser-tests/` (it lives outside the crate directories so `cargo package --workspace` stays warning-free and nothing Node-ish ships to crates.io).

History is linear (rebase, no merge commits). Commit messages are short, complete sentences: capital first letter, trailing period, `backticks` for identifiers. No `Co-Authored-By` trailers. The project name is written `beecast` — all lowercase, even at the start of a sentence; identifiers keep their own casing (`BeeCastPlayer`, `BeeCastVT`).

When updating the first-party player under `player/src/` (this crate is the canonical home; downstream embedders — scsh's session browser among them — consume `beecast-player` from crates.io, so a player change reaches them as a version bump), the `player_bundle_is_inline_safe_and_first_party` test guards the properties every self-contained embedding depends on, and the byte fingerprints in `cli/tests/cli.rs` need re-pinning (the failing assertion prints the new values).

The annotator lives in `.skills/seecast/scripts/seecast.py` — inside the skill directory so `scsh installskills` carries it whole into consumer repos; `seecast/seecast` is a symlink to it. Keep it single-file and stdlib-only.

## Manual verification

The deterministic gate covers parsing, validation, page assembly, and both CLI contracts; two surfaces need a human (or an agent with a browser and credentials) before a release:

- **The generated page's JavaScript.** `cargo run -p beecast -- build cli/tests/fixtures/sample.cast -o /tmp/sample.html`, open it in a browser, and check: chapter buttons seek, speed switching works mid-playback, a `?t=1&note=hi` deep link parks the player with the note banner, and the network tab stays silent throughout.
- **The real annotator path.** With `cursor-agent` logged in, run `./seecast/seecast <recording.cast>` on a real recording: liveness ticks appear on stderr every ~10 s while it works, and the written sidecar passes `./seecast/seecast --validate <recording>.meta.json`. The liberal-acceptance nudge (`beecast <recording>.cast` at a TTY) is checked the same way — it resolves to `build` and prints the canonical spelling in dim text.

Two ENG-PRINCIPLES rules are consciously waived for tools this size — waived, not forgotten: §5's configurable verbosity levels (both CLIs are one-shot and near-silent; stderr diagnostics plus the `--json` documents cover the need), and §3's executable Markdown harness (the deterministic gates above already exercise both tools end to end).
