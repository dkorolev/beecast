# Publishing to crates.io

beecast is a four-crate workspace, and the crates depend on each other, so they must be published **in dependency order** and their versions kept in lockstep. This page is the maintainer playbook.

## Quick reference

Publish the four crates **in this order** (each must be on crates.io before anything that depends on it can build):

```console
$ cargo publish -p beecast-dto      # 1. the metadata DTO — no internal deps    ┐ these two are independent
$ cargo publish -p beecast-player   # 2. the player — no deps at all            ┘ of each other: either order
$ cargo publish -p beecast-page     # 3. depends on beecast-player
$ cargo publish -p beecast          # 4. depends on beecast-dto and beecast-page
```

The order is mandatory, not a convention: `cargo publish` verifies each crate by building it against the **registry**, so `beecast-page` only succeeds once `beecast-player` has landed, and `beecast` once `beecast-dto` and `beecast-page` have. The step-by-step release checklist is [further down](#releasing-step-by-step).

## The dependency graph

```
beecast-dto    (dto/)    — the metadata DTO, no internal deps
beecast-player (player/) — the player, no deps at all
      ▲
      └── beecast-page (page/)  depends on beecast-player
                ▲
   ┌────────────┘
beecast (cli/)  depends on beecast-dto and beecast-page
```

## Where versions live (the reconciliation)

Every crate shares one version, defined in as few places as possible:

| What | Where | How |
| --- | --- | --- |
| Each crate's own version | `dto`, `player`, `page`, `cli` `Cargo.toml` | `version.workspace = true` — inherited, never written per-crate |
| The shared version number | `Cargo.toml` → `[workspace.package] version` | The single source of truth |
| The internal dependency pins | `Cargo.toml` → `[workspace.dependencies]` | `beecast-dto`, `beecast-player`, `beecast-page`, each `{ path = "…", version = "X" }`; the crates reference them as `{ workspace = true }` |

So a release touches exactly **four** literals, all in the root `Cargo.toml`: `[workspace.package] version` and the three internal pins under `[workspace.dependencies]`. Keep them equal.

Why each internal dep carries both `path` and `version`: inside the workspace cargo resolves it by `path`; when publishing, cargo strips the `path` and the published crate depends on the `version`. A path-only dependency cannot be published.

## The packaging gate

Both gates (pre-push and CI — same checks by design, see [`CONTRIBUTING.md`](CONTRIBUTING.md)) run `cargo package --workspace` and require it to be **warning-free**: packaging is the dry run of publishing, and a warning there means the published crates would silently differ from the repo's intent. (Cargo's transient `spurious network error` retry warnings are filtered out — an index hiccup is not a packaging warning.) Packaging the whole workspace at once has no ordering constraint: cargo ≥ 1.90 resolves the internal dependencies against the locally built packages, so this works before anything is on the registry.

To see exactly which files a crate ships:

```console
$ cargo package -p beecast --list
```

## Releasing, step by step

1. **Land all changes and go green.** The full gate must pass and the working tree must be clean (publishing from a dirty tree needs `--allow-dirty` and is discouraged).

2. **Bump the version.** Edit the four literals in the root `Cargo.toml` to the new `X.Y.Z`. Run `cargo build` once so `Cargo.lock` picks up the new version, then commit. (The generated page's footer carries the version, so the byte-pin fingerprints in `cli/tests/cli.rs` need re-pinning with the bump — the failing assertion prints the new values.)

3. **Dry run — verify all four tarballs before anything leaves the machine:**

   ```console
   $ cargo package --workspace
   ```

   This packages and verify-builds all four crates against the local sources. It must finish clean (the gate already requires it warning-free).

4. **Publish in order, waiting for each to land.** Recent cargo waits for the index to update before returning, so the next publish sees its dependency:

   ```console
   $ cargo publish -p beecast-dto
   $ cargo publish -p beecast-player
   $ cargo publish -p beecast-page
   $ cargo publish -p beecast
   ```

   The last command's verification build pulls everything from the registry — exactly what a user's `cargo install beecast` will do — so a green publish confirms the install path too.

5. **Tag and push.**

   ```console
   $ git tag vX.Y.Z && git push --tags
   ```

6. **Verify the published install path.**

   ```console
   $ cargo install beecast --version X.Y.Z
   $ beecast --version
   ```

## Version policy (§7 of the engineering principles)

- Inside the workspace, before a crate's first publish, break freely — no versioning ceremony.
- Once published, a **breaking change bumps the minor version** (`0.x.z` → `0.(x+1).0`), never the patch; patch releases must always be safe to take.
- For `beecast`, the machine-mode `--json` document shapes and the exit-code table are the public surface (§2) — changing what a field or code *means* is a breaking change. For `beecast-player`, the JS API (`BeeCastPlayer` / `BeeCastVT`) and the themable CSS variables are the public surface. For `beecast-dto`, the metadata schema is — its `schema/beecast-meta.schema.json` is pinned byte-for-byte in the gate.
