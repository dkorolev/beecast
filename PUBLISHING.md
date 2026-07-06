# Publishing

The workspace publishes three crates to crates.io. A crate cannot be published until every crate it depends on already exists on the registry at the version it asks for, so **order matters**.

## Order

1. `cargo publish -p beecast-dto` — the cast-metadata DTO, no internal dependencies.
2. `cargo publish -p beecast-page` — the page pipeline, no dependencies at all; independent of `beecast-dto`, so these first two can go in either order.
3. `cargo publish -p beecast` — the CLI, which depends on both and pulls them from the registry.

## One version, one place

The version lives once, in the root `[workspace.package] version`, and all three crates inherit it via `version.workspace = true`. Each internal dependency is declared in `[workspace.dependencies]` with **both** a `path` and a `version`:

```toml
beecast-dto = { path = "dto", version = "0.2.1" }
beecast-page = { path = "page", version = "0.2.1" }
```

Inside the workspace, Cargo resolves them by path. On `cargo publish` it strips the `path` and keeps the `version`, so the published `beecast` depends on `beecast-dto = "0.2.1"` and `beecast-page = "0.2.1"` from crates.io. A path-only dependency cannot be published — the `version` is what makes it publishable.

When bumping the version, change it in exactly three places and keep them equal: `[workspace.package] version` and the `version` in the `beecast-dto` and `beecast-page` entries under `[workspace.dependencies]`.

## Dry run before publishing

`beecast-dto` and `beecast-page` have no internal dependencies, so they verify fully offline:

```
cargo publish -p beecast-dto --dry-run
cargo publish -p beecast-page --dry-run
```

`beecast` depends on both from the registry, so a full dry run (or the real publish) only works **after** they are on crates.io — cargo resolves the version dependencies against the index, not the workspace paths, when preparing the tarball. Before publishing them, verify the CLI's contents (which files ship, tests excluded) with:

```
cargo package -p beecast --list
```

Then the real sequence is: publish `beecast-dto` and `beecast-page`, wait for them to appear on the index, then `cargo publish -p beecast --dry-run` and `cargo publish -p beecast`.

## Versioning discipline (§7)

Until a crate is published, break freely — no versioning ceremony. Once published, a breaking change MUST bump the **minor** version (not the patch): a patch bump is always safe to take, a minor bump means "read the diff." For `beecast`, the machine-mode JSON shape and the exit-code table are the public surface (§2) — changing what a field or code means is a breaking change.
