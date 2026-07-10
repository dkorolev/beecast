# Publishing

The workspace publishes four crates to crates.io. A crate cannot be published until every crate it depends on already exists on the registry at the version it asks for, so **order matters**.

## Order

1. `cargo publish -p beecast-dto` — the cast-metadata DTO, no internal dependencies.
2. `cargo publish -p beecast-player` — the player, no dependencies at all; independent of `beecast-dto`, so these first two can go in either order.
3. `cargo publish -p beecast-page` — the page pipeline, which depends on `beecast-player`.
4. `cargo publish -p beecast` — the CLI, which depends on `beecast-dto` and `beecast-page` and pulls everything from the registry.

## One version, one place

The version lives once, in the root `[workspace.package] version`, and all three crates inherit it via `version.workspace = true`. Each internal dependency is declared in `[workspace.dependencies]` with **both** a `path` and a `version`:

```toml
beecast-dto = { path = "dto", version = "0.3.1" }
beecast-player = { path = "player", version = "0.3.1" }
beecast-page = { path = "page", version = "0.3.1" }
```

Inside the workspace, Cargo resolves them by path. On `cargo publish` it strips the `path` and keeps the `version`, so the published `beecast` depends on `beecast-dto = "0.3.1"` and `beecast-page = "0.3.1"` from crates.io, and the published `beecast-page` on `beecast-player = "0.3.1"`. A path-only dependency cannot be published — the `version` is what makes it publishable.

When bumping the version, change it in exactly four places and keep them equal: `[workspace.package] version` and the `version` in the `beecast-dto`, `beecast-player`, and `beecast-page` entries under `[workspace.dependencies]`.

## Dry run before publishing

`beecast-dto` and `beecast-player` have no internal dependencies, so they verify fully offline:

```
cargo publish -p beecast-dto --dry-run
cargo publish -p beecast-player --dry-run
```

`beecast-page` and `beecast` depend on internal crates from the registry, so a per-crate dry run (or a per-crate publish) only works **after** their dependencies are on crates.io — cargo resolves the version dependencies against the index, not the workspace paths, when preparing the tarball. Packaging the whole workspace at once (`cargo package --workspace`, or a bare `cargo publish`, both cargo ≥ 1.90) has no such ordering constraint: cargo resolves the internal dependencies against the locally built packages. The gate runs `cargo package --workspace` and requires it to be **warning-free** — in particular, the CLI's `include` list ships the test suite and its fixtures, because leaving them out made cargo warn about the auto-discovered test targets. To see exactly which files ship:

```
cargo package -p beecast --list
```

Then the real sequence is: publish `beecast-dto` and `beecast-player`, wait for them to appear on the index, publish `beecast-page`, wait again, then `cargo publish -p beecast --dry-run` and `cargo publish -p beecast`.

## Versioning discipline (§7)

Until a crate is published, break freely — no versioning ceremony. Once published, a breaking change MUST bump the **minor** version (not the patch): a patch bump is always safe to take, a minor bump means "read the diff." For `beecast`, the machine-mode JSON shape and the exit-code table are the public surface (§2) — changing what a field or code means is a breaking change.
