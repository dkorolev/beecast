# Contributing

Two gates, same checks (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test --release`):

1. **Pre-push** — enable once per clone: `git config core.hooksPath .githooks`. Committing locally stays free and fast; the gate runs when code leaves the machine.
2. **CI** — `.github/workflows/ci.yml`, required before merge.

History is linear (rebase, no merge commits). Commit messages are short, complete sentences: capital first letter, trailing period, `backticks` for identifiers. No `Co-Authored-By` trailers.

When re-vendoring `src/vendor/asciinema-player.*`, keep `src/vendor/README.md` accurate; the `vendored_bundle_is_inline_safe` test guards the properties the self-contained page depends on.
