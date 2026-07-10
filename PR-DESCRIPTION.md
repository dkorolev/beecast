## Summary
- Fix a rendering defect in the first-party embedded player: when a TUI paints a multi-row colored panel (an agent CLI's prompt box, a tmux status area, any block with a solid background), playback showed dark lines cutting between the rows instead of one solid block.
- One CSS rule: styled runs render as `display: inline-block`. A plain inline span's background only paints the font's content area (ascent + descent), which is shorter than the 1.25 line box, so every row of a colored panel left an unpainted gap below it. An inline-block's background covers its full border box — the whole line box — so adjacent rows tile seamlessly. Runs never span rows (rows are separate lines in the `<pre>`), so inline-block cannot change any line breaking, and baselines/metrics are unchanged.
- Patch bump to 0.3.1 (§7: safe to take); the byte-pin fingerprints in `cli/tests/cli.rs` are re-pinned accordingly.

## How to Verify
- **Deterministic gate** (same as CI and pre-push): `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo test --workspace --release && python3 -m unittest discover -s seecast/tests && cargo package --workspace` (warning-free).
- **Visually**: build a page from a cast that paints a multi-row background panel (e.g. `printf '\e[48;5;237m%60s\r\n\e[48;5;237m  text\e[K\r\n\e[48;5;237m%60s\e[0m\r\n' '' ''` recorded to a cast), open it, and the panel is one solid block; before the fix, dark gaps ran between its rows. Verified with headless Chrome screenshots of the same cast built with and without the rule.

## Note
The defect dates to the clean-room player's introduction — it replaced asciinema-player, which renders rows as block elements and was immune. Downstream embedders carrying a copy of the player need the same one-line rule until they consume it as a versioned dependency.
