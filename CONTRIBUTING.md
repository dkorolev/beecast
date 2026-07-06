# Contributing

A Cargo workspace with four parts: the [`beecast-dto`](dto) crate (`dto/` — the cast-metadata DTO and the source of truth for the schema), the [`beecast-page`](page) crate (`page/` — the zero-dependency page pipeline: cast inspection and the HTML renderer with the vendored player), the [`beecast`](cli) CLI crate (`cli/` — argument parsing and I/O, depends on both), and the Python `seecast` annotator (`seecast/`). The version is shared once, in the root `[workspace.package]`. Crates publish in dependency order — see [`PUBLISHING.md`](PUBLISHING.md).

`dto/schema/beecast-meta.schema.json` is *generated* from the Rust types in `dto/src/lib.rs` (the source of truth) — regenerate with `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`; a unit test in `beecast-dto` pins the shipped file byte-for-byte, and a Python test cross-checks the facts `validate_meta` mirrors, so drift dies in the gate.

Two gates, same checks (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --workspace --release`, `python3 -m unittest discover -s seecast/tests`):

1. **Pre-push** — enable once per clone: `git config core.hooksPath .githooks`. Committing locally stays free and fast; the gate runs when code leaves the machine.
2. **CI** — `.github/workflows/ci.yml`, required before merge.

History is linear (rebase, no merge commits). Commit messages are short, complete sentences: capital first letter, trailing period, `backticks` for identifiers. No `Co-Authored-By` trailers.

When re-vendoring `page/src/vendor/asciinema-player.*`, keep `page/src/vendor/README.md` accurate; the `vendored_bundle_is_inline_safe` test guards the properties the self-contained page depends on, and the byte fingerprints in `cli/tests/cli.rs` need re-pinning (the failing assertion prints the new values).

The annotator lives in `.skills/seecast/scripts/seecast.py` — inside the skill directory so `scsh installskills` carries it whole into consumer repos; `seecast/seecast` is a symlink to it. Keep it single-file and stdlib-only.

## Manual verification

The deterministic gate covers parsing, validation, page assembly, and both CLI contracts; two surfaces need a human (or an agent with a browser and credentials) before a release:

- **The generated page's JavaScript.** `cargo run -p beecast -- build cli/tests/fixtures/sample.cast -o /tmp/sample.html`, open it in a browser, and check: chapter buttons seek, speed switching works mid-playback, a `?t=1&note=hi` deep link parks the player with the note banner, and the network tab stays silent throughout.
- **The real annotator path.** With `cursor-agent` logged in, run `./seecast/seecast <recording.cast>` on a real recording: liveness ticks appear on stderr every ~10 s while it works, and the written sidecar passes `./seecast/seecast --validate <recording>.meta.json`. The liberal-acceptance nudge (`beecast <recording>.cast` at a TTY) is checked the same way — it resolves to `build` and prints the canonical spelling in dim text.

Two ENG-PRINCIPLES rules are consciously waived for tools this size — waived, not forgotten: §5's configurable verbosity levels (both CLIs are one-shot and near-silent; stderr diagnostics plus the `--json` documents cover the need), and §3's executable Markdown harness (the deterministic gates above already exercise both tools end to end).
