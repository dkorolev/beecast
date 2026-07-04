# Contributing

One repo, two deliverables: the Rust `beecast` renderer (repo root) and the Python `seecast` annotator (`seecast/`), sharing one schema (`schema/beecast-meta.schema.json`, rendered from the Rust types in `src/meta.rs` — the source of truth).

Two gates, same checks (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test --release`, `python3 -m unittest discover -s seecast/tests`):

1. **Pre-push** — enable once per clone: `git config core.hooksPath .githooks`. Committing locally stays free and fast; the gate runs when code leaves the machine.
2. **CI** — `.github/workflows/ci.yml`, required before merge.

History is linear (rebase, no merge commits). Commit messages are short, complete sentences: capital first letter, trailing period, `backticks` for identifiers. No `Co-Authored-By` trailers.

When re-vendoring `src/vendor/asciinema-player.*`, keep `src/vendor/README.md` accurate; the `vendored_bundle_is_inline_safe` test guards the properties the self-contained page depends on.

The annotator lives in `.skills/seecast/scripts/seecast.py` — inside the skill directory so `scsh installskills` carries it whole into consumer repos; `seecast/seecast` is a symlink to it. Keep it single-file and stdlib-only.
