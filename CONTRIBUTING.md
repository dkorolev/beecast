# Contributing

A Cargo workspace with three parts: the [`beecast-dto`](dto) crate (`dto/` — the cast-metadata DTO and the source of truth for the schema), the [`beecast`](cli) CLI crate (`cli/` — the renderer, depends on `beecast-dto`), and the Python `seecast` annotator (`seecast/`). The version is shared once, in the root `[workspace.package]`. Crates publish in dependency order — see [`PUBLISHING.md`](PUBLISHING.md).

`dto/schema/beecast-meta.schema.json` is *generated* from the Rust types in `dto/src/lib.rs` (the source of truth) — regenerate with `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`; a unit test in `beecast-dto` pins the shipped file byte-for-byte, and a Python test cross-checks the facts `validate_meta` mirrors, so drift dies in the gate.

Two gates, same checks (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --workspace --release`, `python3 -m unittest discover -s seecast/tests`):

1. **Pre-push** — enable once per clone: `git config core.hooksPath .githooks`. Committing locally stays free and fast; the gate runs when code leaves the machine.
2. **CI** — `.github/workflows/ci.yml`, required before merge.

History is linear (rebase, no merge commits). Commit messages are short, complete sentences: capital first letter, trailing period, `backticks` for identifiers. No `Co-Authored-By` trailers.

When re-vendoring `cli/src/vendor/asciinema-player.*`, keep `cli/src/vendor/README.md` accurate; the `vendored_bundle_is_inline_safe` test guards the properties the self-contained page depends on.

The annotator lives in `.skills/seecast/scripts/seecast.py` — inside the skill directory so `scsh installskills` carries it whole into consumer repos; `seecast/seecast` is a symlink to it. Keep it single-file and stdlib-only.

Two ENG-PRINCIPLES rules are consciously waived for tools this size — waived, not forgotten: §5's configurable verbosity levels (both CLIs are one-shot and near-silent; stderr diagnostics plus the `--json` documents cover the need), and §3's executable Markdown harness (the deterministic gates above already exercise both tools end to end).
